//! Bounded, MEMORY-BUDGETED parallelism for the per-photo CLI loops (`batch`,
//! `eval`) — the `--jobs N` machinery, plus the output sequencer that keeps a
//! parallel run's report readable.
//!
//! # Why a memory budget and not just a worker count (R27 Batch-7)
//!
//! One photo's non-network pipeline pass peaks at **~1.77 GB of COMMIT charge**
//! on this project's reference corpus (61 MP Sony ARW). Measured 2026-08-19 on
//! 12 RAW+`.xmp` pairs from the eval corpus, replaying exactly what
//! `pipeline::produce_recipe` runs between the network calls, and reading the
//! process's own `PeakPagefileUsage` between photos:
//!
//! ```text
//!   stage=decode  (decode_any + preview_resized)      peak commit   151 MB
//!   stage=cal     (photo_base_knots -> render_to_image) peak commit 1771 MB
//!   both                                              peak commit 1771 MB
//! ```
//!
//! The 1.77 GB lives in the demosaic: `render::render_to_image` produces the
//! full-frame oriented f32 buffer (61 MP x 3 x 4 B ~ 732 MB, plus rawler's own
//! sensor buffers and the orientation transient) BEFORE it applies `max_edge`
//! (the `match max_edge` in `render::render_to_image_in`), so `photo_base_knots`' "capped at 2048" bounds the
//! develop stages but NOT the peak.
//!
//! ## The render tail, measured (R28 Batch-4)
//!
//! What R27 did NOT measure — and what the R28 adjudication (F2) registered as
//! the one UNVERIFIABLE part of the constant — is that `batch --render` also
//! pays `render::render_to_file` at FULL resolution afterwards. That is now
//! measured, by the same method, on the same 60.2 MP A7R V frame
//! (9504x6336), release profile, ONE STAGE PER PROCESS so no row inherits an
//! earlier row's high-water mark (`tests::probe_per_photo_peak_commit`,
//! 2026-08-20):
//!
//! ```text
//!   stage=decode  alone                               peak commit   151 MB
//!   stage=cal     alone                               peak commit  1771 MB
//!   stage=render  alone (render_to_file, max_edge=None,
//!                 clarity/texture/sharpen/NR all on)  peak commit  1766 MB
//!   all three, one process                            peak commit  1771 MB
//! ```
//!
//! **The tail does not raise the peak, and 1,800 MB stands** — but only just.
//! The two big stages land 5 MB apart (1771 vs 1766) because they share the
//! expensive moment: BOTH run the same full-frame demosaic and orientation,
//! and `max_edge` only decides what happens after it. The full-resolution
//! render's own extra — the 16-bit pack beside the f32 plane — is smaller than
//! the working-resolution path's orientation transient, so the tail comes in
//! marginally UNDER the stage that was already measured. The `all` row equals
//! the `cal` row, which is the same fact read from the other side: no stage
//! stacks on another, because each frees its buffers before the next
//! allocates.
//!
//! The margin is thin and worth saying so: 1,800 is a round-UP of 1,771, not a
//! safety factor. It holds for this corpus (one body, one CFA); a decoder that
//! peaked higher per pixel would eat it. What bounds THAT case is the
//! per-file ceiling this batch added at the develop door, not this constant.
//!
//! Per source pixel that peak is 30.8 B, which is where
//! `decode::RAW_DEVELOP_BYTES_PER_PIXEL` (31, rounded UP) comes from — the
//! per-FILE half of the same accounting, added in this batch because the
//! constant below is deliberately per-corpus and a single 150 MP file can
//! exceed it on its own.
//!
//! Luminance-return follow-up (2026-09-20): the same 60.2 MP render probe,
//! with a CPU stand-in publishing a captured clean mosaic, measured 1765 MiB
//! / 5.131 s at denoise strength 1 and 1996 MiB / 10.247 s at 0.71, each in a
//! fresh process. The extra Y plane raises commit by 231 MiB. These figures
//! exclude the sidecar process. Batch render requests no AI denoise, so its
//! ordinary 1800 MB budget still describes its path. The opt-in RAW return
//! has its own conservative per-file ceiling allowance in decode.rs.
//!
//! What the same measurement REFUTES is accumulation. Over 11 consecutive
//! photos the peak-commit high-water mark moved 1771.3 -> 1772.3 MB (+1.0 MB
//! total) and the resident commit BETWEEN photos sat at 12.2-12.6 MB. A leak
//! would push the high-water mark up every iteration; it does not. The
//! 147-photo decode-only probe agrees — flat at 112-127 MB mean commit across
//! all four quartiles of the run. So the failure mode the `--jobs` cap has to
//! defend against is PEAK, hit once per photo and multiplied by every worker,
//! not growth.
//!
//! That is why the cap is derived from free memory rather than from CPU count:
//! `available_parallelism()` on an 8-core box would authorise ~14 GB of
//! concurrent commit for work that is anyway dominated by network latency.
//!
//! # Output discipline
//!
//! Workers never print. Each writes its whole per-photo block into a `String`
//! and hands it to the [`Sequencer`], which releases block `i` only once every
//! block before it has been written — so a parallel run's transcript is
//! byte-identical in ORDER to a serial one, whatever order the photos actually
//! finish in. (The cost is latency, not correctness: a slow photo 1 holds the
//! finished blocks of 2..k until it lands.)
//!
//! # API concurrency, stated plainly
//!
//! There is NO rate limiter here. `N` workers means up to `N` proposer calls in
//! flight, and that is the whole of the throttling — each call keeps the retry
//! and timeout budget it already had (`AUTOSHADE_HTTP_TIMEOUT_SECS`), and nothing
//! coordinates between them. The endpoint these calls now reach is a relay whose
//! requests-per-minute ceiling is unknown to this codebase, so a global limiter
//! would have to invent a number; the honest bound is the worker count the user
//! chose. If a run starts collecting 429s, lower `--jobs`.
//!
//! The DEVELOP CHAIN's own disclosures are inside this ordering. They used to
//! be bare `eprintln!`s that went straight to the process stderr in COMPLETION
//! order — the GPT-proposer fallback warn, the clamp disclosure, the XMP merge
//! notes. `produce_recipe`, `render_to_file` and `write_xmp` now take a
//! caller-supplied [`crate::diag::Sink`], `batch` gives each worker a
//! `diag::Collector` and drains it into THAT photo's block, and v1.2.4
//! finished the sweep: `pipeline.rs`, `render.rs` and `recipe.rs` hold zero
//! bare `eprintln!` and the census in `pipeline::tests` pins that at zero. So
//! those lines are released by the sequencer in INDEX order with the rest of
//! the transcript — on stdout, inside the block, attributable by position.
//!
//! Not covered, and disclosed rather than pretended away: bare `eprintln!`s a
//! worker can transitively reach that are NOT the develop chain's — the
//! store's and the decoder's. Those still go to the process stderr and
//! therefore still appear in COMPLETION order. They cannot interleave mid-line
//! — Rust holds the stderr lock across the whole `write_fmt` — but they are
//! not attributable to a photo by position. The typed `rationale::Note`
//! channel `produce_recipe` returns as its third element remains the
//! attributable path for anything a caller renders itself (that is what `eval`
//! does above one job).

use std::collections::BTreeMap;
use std::io::Write;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Peak COMMIT charge, in MB, that ONE photo's pipeline pass reaches.
///
/// Provenance: measured 2026-08-19 (R27 Batch-7), RE-MEASURED and extended
/// through `render_to_file` 2026-08-20 (R28 Batch-4) — see the module docs for
/// both per-stage tables and the method, and `tests::probe_per_photo_peak_commit`
/// for the harness. Rounded UP from the observed 1771 MB, since the budget it
/// divides is a safety margin and a low estimate over-subscribes.
///
/// It is a property of the CORPUS as much as of the code (a 24 MP body peaks
/// far lower), so it is deliberately the pessimistic end of the libraries this
/// project is used on. What it is NOT is a per-file figure, and R27's reason
/// for that — "reading each RAW's dimensions would cost a decode per photo" —
/// was only ever true for RAWs. [`survey_peak_mb`] now asks the BAKED sources
/// on the work list, whose headers are free, and RAWs are bounded per-file at
/// the develop door instead (`decode::refuse_raw_develop_over_ceiling`). This
/// constant is the floor of that pair, not the whole of it.
pub const PER_PHOTO_PEAK_COMMIT_MB: u64 = 1_800;

/// Share of the machine's free memory the pool may commit to photos, as a
/// percentage. Half: the other half is the rest of the machine — a browser, an
/// editor, and (on the `eval` path) the `claude` verifier subprocess this
/// process spawns per photo, which is a Node runtime of its own and is NOT
/// counted in [`PER_PHOTO_PEAK_COMMIT_MB`].
const BUDGET_PCT: u64 = 50;

/// The per-photo peak THIS run must budget for, plus the disclosure that
/// belongs with it — [`PER_PHOTO_PEAK_COMMIT_MB`] unless the work list's own
/// headers say a file on it is bigger than the corpus default.
///
/// **This is the dimensionless-constant fix (R28 Batch-4 4a; adjudication
/// F2).** The constant is a property of the corpus, not of the files in front
/// of it, and nothing re-checked it against them: four 143 MP exports on a
/// 16 GB machine planned four workers against a 7.2 GB budget and asked for
/// ~16 GB, silently. The planner now ASKS, for every source whose header is
/// cheap to read.
///
/// Deliberately asymmetric, and that asymmetry is the honest part:
///
/// * **Baked** sources answer ([`crate::decode::cheap_develop_peak_mb`]) — a
///   header parse, no pixel decoded. These are also the reachable overshoot:
///   Lightroom's "Edit in…" writes native-resolution 16-bit TIFFs, and
///   `batch --include-baked` develops them.
/// * **Camera RAWs** answer `None`, because asking would map the whole file
///   per photo before the pool starts. They are bounded at the develop door
///   instead, by the per-file 4 GiB ceiling this batch added
///   (`decode::refuse_raw_develop_over_ceiling`), which did not exist when F2
///   was written. Between the two, no admitted file can exceed 4 GiB and no
///   surveyed file is budgeted at less than it needs.
///
/// The MAXIMUM, not the mean: the budget divides into concurrent workers that
/// can each be handed the largest photo, and a mean would authorise exactly
/// the overshoot this exists to stop.
pub fn survey_peak_mb(work: &[&std::path::Path]) -> (u64, Option<String>) {
    survey_peak_with(work, crate::decode::cheap_develop_peak_mb)
}

/// [`survey_peak_mb`] with the per-file estimate INJECTED — the same
/// discipline [`plan_with`] applies to the memory reading, and for the same
/// reason: the decision (max, threshold, disclosure) is then testable without
/// a 64 MP file on disk to produce a number with.
fn survey_peak_with(
    work: &[&std::path::Path],
    estimate: impl Fn(&std::path::Path) -> Option<u64>,
) -> (u64, Option<String>) {
    let mut worst: Option<(u64, &std::path::Path)> = None;
    for p in work {
        if let Some(mb) = estimate(p)
            && mb > worst.map_or(0, |(v, _)| v)
        {
            worst = Some((mb, p));
        }
    }
    match worst {
        Some((mb, p)) if mb > PER_PHOTO_PEAK_COMMIT_MB => (
            mb,
            // Says what was found, not what will follow: the cap note below
            // only prints when the budget actually overrules the flag, so
            // promising a smaller worker count here would be a claim this
            // function cannot keep on a machine with memory to spare.
            Some(format!(
                "  memory budget: {} alone peaks at ~{mb} MB — more than the \
                 {PER_PHOTO_PEAK_COMMIT_MB} MB this corpus usually needs, so this run is \
                 budgeted at {mb} MB per photo.",
                p.display()
            )),
        ),
        _ => (PER_PHOTO_PEAK_COMMIT_MB, None),
    }
}

/// Free memory this machine can hand out right now, in MB, or `None` when the
/// OS cannot be asked (then no memory cap applies and `--jobs` is taken at its
/// word — a wrong guess must not silently serialise the user's run).
///
/// The MINIMUM of free physical memory and free commit, because either one
/// running out ends the run: exhausting commit fails the allocation outright,
/// while committing far past physical memory just moves the 1.8 GB peak onto
/// the pagefile and turns a 2-second demosaic into a disk-bound crawl.
pub fn free_memory_mb() -> Option<u64> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::SystemInformation::{
            GlobalMemoryStatusEx, MEMORYSTATUSEX,
        };
        let mut m: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
        m.dwLength = size_of::<MEMORYSTATUSEX>() as u32;
        // 0 = failure; the struct is then untouched and must not be read.
        if unsafe { GlobalMemoryStatusEx(&mut m) } == 0 {
            return None;
        }
        Some(m.ullAvailPhys.min(m.ullAvailPageFile) / (1024 * 1024))
    }
    // `_SC_AVPHYS_PAGES` is a Linux/glibc name; the other unixes either lack it
    // or report something else entirely, and guessing there would be the
    // "confident wrong answer" this cap exists to avoid. They get `None` (no
    // cap), which is exactly the pre-R27 behaviour.
    #[cfg(target_os = "linux")]
    {
        let pages = unsafe { libc::sysconf(libc::_SC_AVPHYS_PAGES) };
        let size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        if pages <= 0 || size <= 0 {
            return None;
        }
        Some((pages as u64).saturating_mul(size as u64) / (1024 * 1024))
    }
    #[cfg(target_os = "macos")]
    {
        // Two questions, two APIs. `hw.memsize` is the machine's physical RAM
        // — the ceiling, the same role `ullAvailPhys` bounds on Windows — and
        // `host_statistics64` reports what is actually reclaimable.
        //
        // free + INACTIVE, not free alone, and that is the whole subtlety of
        // this platform: macOS parks clean file-backed pages in `inactive` and
        // evicts them on demand, so a healthy Mac reports almost nothing free
        // by design. Counting only `free_count` would cap `--jobs` at 1 on a
        // 64 GB machine whose memory is doing exactly what it should. This is
        // also why `_SC_AVPHYS_PAGES` is not used here even though Darwin
        // defines the name: it reports that same under-count.
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        if page <= 0 {
            return None;
        }
        let mut total: u64 = 0;
        let mut len = size_of::<u64>();
        // SAFETY: `hw.memsize` is a u64 sysctl; `len` describes the buffer
        // handed in, and a non-zero return is treated as "no answer" below
        // rather than trusting whatever `total` holds.
        let got_total = unsafe {
            libc::sysctlbyname(
                c"hw.memsize".as_ptr(),
                (&raw mut total).cast(),
                &raw mut len,
                std::ptr::null_mut(),
                0,
            )
        } == 0;
        if !got_total || total == 0 {
            return None;
        }
        // `libc::mach_host_self` is deprecated in favour of the `mach2`
        // crate. This is the same one symbol out of libSystem, which every
        // macOS process links already — the house raw-link pattern (see the
        // GUI's crash dialog), not a new dependency bought for one call.
        unsafe extern "C" {
            fn mach_host_self() -> libc::mach_port_t;
        }
        let mut vm: libc::vm_statistics64 = unsafe { std::mem::zeroed() };
        let mut count = libc::HOST_VM_INFO64_COUNT;
        // SAFETY: flavour, buffer and count agree — `HOST_VM_INFO64_COUNT` is
        // libc's own element count for exactly this struct.
        let kr = unsafe {
            libc::host_statistics64(
                mach_host_self(),
                libc::HOST_VM_INFO64,
                (&raw mut vm).cast(),
                &raw mut count,
            )
        };
        if kr != 0 {
            return None;
        }
        let pages = u64::from(vm.free_count).saturating_add(u64::from(vm.inactive_count));
        let avail = pages.saturating_mul(page as u64);
        Some(avail.min(total) / (1024 * 1024))
    }
    #[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
    {
        None
    }
}

/// How many concurrent photos `headroom_mb` of free memory pays for, or `None`
/// when the machine could not be measured. Never 0: one photo at a time is the
/// serial behaviour, and refusing to run at all because memory is tight would
/// be worse than running slowly.
/// Saturating like [`free_memory_mb`]'s own Linux arm (`saturating_mul` at the
/// page-count multiply): this is a public `lib` entry point via [`plan_with`],
/// so the reading is not always one this module produced. `plan()`'s own path
/// cannot overflow by construction — both `free_memory_mb` arms divide by 2^20
/// last, leaving four orders of magnitude of headroom under `×50` — but an
/// injected `u64::MAX` panicked here under debug overflow checks while the
/// identical hazard one function up was already handled (R28 2d, adjudication
/// F10). Saturating then dividing keeps the arithmetic honest at the top: a
/// saturated budget still divides down to a worker count, and the `.max(1)`
/// below means no reading can ever produce "run nothing".
fn memory_cap(headroom_mb: Option<u64>, per_photo_peak_mb: u64) -> Option<usize> {
    let budget = headroom_mb?.saturating_mul(BUDGET_PCT) / 100;
    // `.max(1)` on the divisor too: `per_photo_peak_mb` reaches here from
    // [`survey_peak_mb`], and a header that answered 0 MB would otherwise
    // divide by zero. The constant itself can never be 0.
    Some(((budget / per_photo_peak_mb.max(1)) as usize).max(1))
}

/// The decided worker count plus the DISCLOSURE the caller must print when the
/// memory budget overruled what the user asked for. Silently running fewer
/// workers than `--jobs N` would look like the flag did nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub jobs: usize,
    pub note: Option<String>,
}

/// Resolve `--jobs` against the work list, the machine's free memory, and the
/// files' OWN per-photo peaks: the divisor is the larger of the corpus
/// constant and what these files' headers say ([`survey_peak_mb`]), and both
/// disclosures ride out together.
///
/// THE door. There was a count-only `plan(requested, work: usize)` beside it
/// until R28 Batch-4; it is gone rather than kept, because a shorter name that
/// silently skips the per-file survey is the footgun this batch exists to
/// remove — the whole finding (adjudication F2) was that the budget never
/// looked at the files it was budgeting for.
pub fn plan_for(requested: usize, work: &[&std::path::Path]) -> Plan {
    let (peak, surveyed) = survey_peak_mb(work);
    let mut plan = plan_with_peak(requested, work.len(), free_memory_mb(), peak);
    // BOTH lines, never one: the survey note explains where the divisor came
    // from and the cap note explains what it cost, and printing only the
    // second would leave "not 4" looking arbitrary on a machine with plenty of
    // memory for the corpus default.
    plan.note = match (surveyed, plan.note.take()) {
        (Some(a), Some(b)) => Some(format!("{a}\n{b}")),
        (a, b) => a.or(b),
    };
    plan
}

/// [`plan_for`] with the memory reading injected and the corpus constant taken
/// as the per-photo peak — the whole decision as a pure function, so the cap is
/// testable without a machine in a particular state or files on disk.
///
/// A TEST SEAM, not a second door: it cannot consult the work list because it
/// is not given one. Production callers take [`plan_for`].
pub fn plan_with(requested: usize, work: usize, headroom_mb: Option<u64>) -> Plan {
    plan_with_peak(requested, work, headroom_mb, PER_PHOTO_PEAK_COMMIT_MB)
}

/// [`plan_with`] with the per-photo peak injected too — the whole decision,
/// including the R28 per-file half, as a pure function.
pub fn plan_with_peak(
    requested: usize,
    work: usize,
    headroom_mb: Option<u64>,
    per_photo_peak_mb: u64,
) -> Plan {
    // 0 is a user typo, not "no workers"; more workers than photos is waste.
    let asked = requested.max(1);
    let by_work = asked.min(work.max(1));
    match memory_cap(headroom_mb, per_photo_peak_mb) {
        Some(cap) if cap < by_work => Plan {
            jobs: cap,
            note: Some(format!(
                "  memory budget: running {cap} worker(s), not {asked} — one photo peaks at \
                 ~{per_photo_peak_mb} MB and only {} MB is free ({BUDGET_PCT}% of it \
                 budgeted for this run).",
                headroom_mb.unwrap_or(0)
            )),
        },
        _ => Plan { jobs: by_work, note: None },
    }
}

/// Releases per-photo output blocks in INDEX order, whatever order the workers
/// finish in.
///
/// A completed block whose turn has not come is HELD, not waited on: parking
/// the worker would idle a whole pipeline slot behind one slow photo. At most
/// one block per not-yet-released index is buffered, and a block is a few
/// hundred bytes of text.
pub struct Sequencer {
    state: Mutex<SeqState>,
}

struct SeqState {
    /// The index whose block may be written next.
    next: usize,
    held: BTreeMap<usize, String>,
}

impl Default for Sequencer {
    fn default() -> Self {
        Self::new()
    }
}

impl Sequencer {
    pub fn new() -> Self {
        Self { state: Mutex::new(SeqState { next: 0, held: BTreeMap::new() }) }
    }

    /// Hand `index`'s finished block over, then write out every block that has
    /// now become releasable.
    ///
    /// Poison is recovered rather than re-panicked: one worker panicking must
    /// not turn every OTHER worker's release into a second panic, which inside
    /// `thread::scope` would abort instead of unwinding to a joinable end.
    pub fn release(&self, index: usize, block: String) {
        let mut guard = self.state.lock().unwrap_or_else(|p| p.into_inner());
        // Split the guard once: `held` and `next` are two fields of one struct
        // reached through a DerefMut, which cannot be borrowed apart inline.
        let st = &mut *guard;
        st.held.insert(index, block);
        let ready = st.drain_ready();
        if ready.is_empty() {
            return;
        }
        // stdout taken INSIDE the state lock, and only here: two releases can
        // never write between each other's lines. No other path in this module
        // takes the two locks, so the nesting cannot invert.
        let mut out = std::io::stdout().lock();
        for text in ready {
            let _ = out.write_all(text.as_bytes());
        }
        let _ = out.flush();
    }
}

impl SeqState {
    fn drain_ready(&mut self) -> Vec<String> {
        let mut ready = Vec::new();
        while let Some(text) = self.held.remove(&self.next) {
            ready.push(text);
            self.next += 1;
        }
        ready
    }
}

/// Run `body` over `0..n` on at most `jobs` threads, releasing each index's
/// output block in index order and returning every index's result in index
/// order.
///
/// `body` writes its lines into the `&mut String` it is handed; it must NOT
/// print, or the sequencer's ordering guarantee is void for those lines.
///
/// The results come back positionally so the caller can FOLD them in index
/// order — which is what keeps a parallel aggregate bit-identical to the serial
/// one, since summing f64s in completion order would drift in the last ulp.
///
/// A panic inside `body` propagates out of the scope, as it did in the
/// hand-rolled pool this replaces (main.rs' batch loop): the blocks of indices
/// that completed after the panicking one are then forfeited with the run.
pub fn for_each_indexed<T, F>(jobs: usize, n: usize, body: F) -> Vec<Option<T>>
where
    F: Fn(usize, &mut String) -> T + Sync,
    T: Send,
{
    let jobs = jobs.max(1).min(n.max(1));
    let next = AtomicUsize::new(0);
    let out: Mutex<Vec<Option<T>>> = Mutex::new((0..n).map(|_| None).collect());
    let seq = Sequencer::new();
    std::thread::scope(|s| {
        for _ in 0..jobs {
            s.spawn(|| {
                loop {
                    // The shared counter over a FIXED list preserves `--limit`
                    // semantics exactly (the rule the batch pool already had).
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= n {
                        break;
                    }
                    let mut block = String::new();
                    let value = body(i, &mut block);
                    // The result lands BEFORE the block is released, so a
                    // caller reading results after the scope can never see a
                    // printed photo whose result is still missing.
                    out.lock().unwrap_or_else(|p| p.into_inner())[i] = Some(value);
                    seq.release(i, block);
                }
            });
        }
    });
    out.into_inner().unwrap_or_else(|p| p.into_inner())
}
