use std::io::BufRead;
use std::process::Stdio;
use std::time::{Duration, Instant};

#[cfg(unix)]
#[test]
fn killing_a_sidecar_group_reaps_its_grandchild() {
    let mut command = autoshade::sidecar_command("sh");
    command
        .env_clear()
        .arg("-c")
        .arg("exec 3<&0; cat <&3 >/dev/null & echo $!; wait")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    autoshade::arm_kill_group(&mut command);
    let mut child = autoshade::spawn_child(&mut command).unwrap();
    // Hold the input open so the descendant cannot finish before cancellation.
    let _input = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut line = String::new();
    std::io::BufReader::new(stdout)
        .read_line(&mut line)
        .unwrap();
    let grandchild = line.trim().parse::<u32>().unwrap();
    let group = autoshade::assign_kill_group(&child).expect("the child is its own process group");
    let descendant_alive = || {
        // SAFETY: signal 0 only queries the child PID's existence.
        unsafe { libc::kill(grandchild as i32, 0) == 0 }
    };
    assert!(
        descendant_alive(),
        "the descendant must exist before cancellation"
    );

    group.kill_tree();
    let deadline = Instant::now() + Duration::from_secs(2);
    while child.try_wait().unwrap().is_none() {
        assert!(
            Instant::now() < deadline,
            "the sidecar leader survived cancellation"
        );
        std::thread::yield_now();
    }
    let alive = loop {
        if !descendant_alive() {
            break false;
        }
        if Instant::now() >= deadline {
            break true;
        }
        std::thread::yield_now();
    };
    assert!(!alive, "grandchild {grandchild} survived kill_tree");
}
