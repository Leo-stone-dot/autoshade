mod support;

use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use autoshade::jobs::{
    PER_PHOTO_PEAK_COMMIT_MB, Plan, Sequencer, for_each_indexed, plan_with_peak, survey_peak_mb,
};
use support::TempTree;

fn png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc = crc32fast::Hasher::new();
    crc.update(kind);
    crc.update(data);
    out.extend_from_slice(&crc.finalize().to_be_bytes());
}

fn write_header_only_png(path: &std::path::Path, width: u32, height: u32) {
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[16, 2, 0, 0, 0]);
    png_chunk(&mut png, b"IHDR", &ihdr);
    png_chunk(
        &mut png,
        b"IDAT",
        &[0x78, 0x9c, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01],
    );
    png_chunk(&mut png, b"IEND", &[]);
    std::fs::write(path, png).unwrap();
}

#[allow(clippy::disallowed_methods)] // captures the public sequencer's process stdout
fn run_fixture(name: &str) -> std::process::Output {
    Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", name, "--nocapture"])
        .output()
        .unwrap()
}

#[test]
fn sequencer_releases_completed_blocks_in_index_order() {
    let output = run_fixture("sequencer_out_of_order_fixture");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let positions = [0, 1, 2, 3].map(|index| {
        stdout
            .find(&format!("AUTOSHADE_SEQUENCE_{index}"))
            .unwrap_or_else(|| panic!("missing index {index}: {stdout}"))
    });

    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "{stdout}"
    );
}

#[test]
fn sequencer_holds_a_block_until_every_earlier_index_arrives() {
    let output = run_fixture("sequencer_held_block_fixture");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(!stdout.contains("AUTOSHADE_HELD_BLOCK"), "{stdout}");
}

#[ignore = "subprocess fixture invoked by sequencer_releases_completed_blocks_in_index_order"]
#[test]
fn sequencer_out_of_order_fixture() {
    let sequencer = Sequencer::new();
    for index in [3, 1, 2, 0] {
        sequencer.release(index, format!("AUTOSHADE_SEQUENCE_{index}\n"));
    }
}

#[ignore = "subprocess fixture invoked by sequencer_holds_a_block_until_every_earlier_index_arrives"]
#[test]
fn sequencer_held_block_fixture() {
    Sequencer::new().release(7, "AUTOSHADE_HELD_BLOCK\n".to_string());
}

#[test]
fn parallel_pool_returns_results_at_their_input_indices() {
    let results = for_each_indexed(4, 64, |index, _| index * 2);

    assert_eq!(
        results,
        (0..64).map(|index| Some(index * 2)).collect::<Vec<_>>()
    );
}

#[test]
fn parallel_pool_visits_every_index_exactly_once() {
    let seen = (0..64).map(|_| AtomicUsize::new(0)).collect::<Vec<_>>();

    let _ = for_each_indexed(4, seen.len(), |index, _| {
        seen[index].fetch_add(1, Ordering::Relaxed);
    });

    assert!(seen.iter().all(|count| count.load(Ordering::Relaxed) == 1));
}

#[test]
fn one_worker_visits_indices_in_strict_order() {
    let order = Mutex::new(Vec::new());

    let _ = for_each_indexed(1, 20, |index, _| {
        order.lock().unwrap().push(index);
    });

    assert_eq!(order.into_inner().unwrap(), (0..20).collect::<Vec<_>>());
}

#[test]
fn memory_budget_uses_half_the_headroom_and_the_declared_peak() {
    for (headroom, peak, expected) in [
        (Some(16_384), 1_800, 4),
        (Some(8_192), 1_800, 2),
        (Some(512), 1_800, 1),
        (Some(0), 1_800, 1),
        (Some(16_384), 4_095, 2),
    ] {
        assert_eq!(
            plan_with_peak(100, 100, headroom, peak).jobs,
            expected,
            "headroom={headroom:?} peak={peak}"
        );
    }
}

#[test]
fn an_unmeasurable_machine_does_not_invent_a_memory_cap() {
    assert_eq!(
        plan_with_peak(8, 10, None, PER_PHOTO_PEAK_COMMIT_MB),
        Plan {
            jobs: 8,
            note: None,
        }
    );
}

#[test]
fn worker_plan_takes_the_smallest_of_request_work_and_memory() {
    assert_eq!(plan_with_peak(4, 10, Some(64_000), 1_800).jobs, 4);
    assert_eq!(plan_with_peak(8, 3, Some(64_000), 1_800).jobs, 3);
    assert_eq!(plan_with_peak(8, 100, Some(8_192), 1_800).jobs, 2);
}

#[test]
fn zero_request_and_zero_work_still_plan_one_worker() {
    assert_eq!(plan_with_peak(0, 10, Some(64_000), 1_800).jobs, 1);
    assert_eq!(plan_with_peak(4, 0, None, 1_800).jobs, 1);
}

#[test]
fn a_memory_downgrade_names_the_request_headroom_and_peak() {
    let plan = plan_with_peak(8, 100, Some(8_192), 1_800);
    let note = plan.note.expect("a reduced worker count must be disclosed");

    assert!(note.contains("not 8"), "note={note}");
    assert!(note.contains("8192"), "note={note}");
    assert!(note.contains("1800"), "note={note}");
}

#[test]
fn an_absurd_memory_reading_plans_without_overflowing() {
    assert_eq!(
        plan_with_peak(1, 10, Some(u64::MAX), 1_800),
        Plan {
            jobs: 1,
            note: None,
        }
    );
    for headroom in [u64::MAX, u64::MAX / 2, u64::MAX / 50 + 1] {
        assert!(plan_with_peak(100, 100, Some(headroom), 1_800).jobs >= 1);
    }
}

#[test]
fn survey_uses_the_largest_baked_header_and_names_it() {
    let tree = TempTree::new("jobs-survey-largest");
    let small = tree.path().join("small.png");
    let huge = tree.path().join("huge.png");
    let raw = tree.path().join("camera.arw");
    write_header_only_png(&small, 100, 100);
    write_header_only_png(&huge, 10_000, 8_000);

    let (peak, note) = survey_peak_mb(&[raw.as_path(), small.as_path(), huge.as_path()]);
    let note = note.expect("the huge baked source must raise the corpus default");

    assert!(peak > PER_PHOTO_PEAK_COMMIT_MB, "peak={peak}");
    assert!(note.contains("huge.png"), "note={note}");
    assert!(note.contains(&peak.to_string()), "note={note}");
}

#[test]
fn survey_keeps_the_corpus_default_when_no_header_exceeds_it() {
    let tree = TempTree::new("jobs-survey-default");
    let small = tree.path().join("small.png");
    let raw = tree.path().join("camera.arw");
    write_header_only_png(&small, 100, 100);

    assert_eq!(
        survey_peak_mb(&[raw.as_path(), small.as_path()]),
        (PER_PHOTO_PEAK_COMMIT_MB, None)
    );
    assert_eq!(survey_peak_mb(&[]), (PER_PHOTO_PEAK_COMMIT_MB, None));
}
