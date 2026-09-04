#[cfg(windows)]
#[tokio::test]
async fn test_windows_job_object_isolation() {
    use intermcp::reaper::{windows::ProcessJobGroup, ChildIsolationGuard};
    use std::ptr::null_mut;
    use tokio::process::Command;

    // 1. Assigning an invalid handle fails
    let job = ProcessJobGroup::new().expect("Create Job Object on Windows");
    let assign_invalid = unsafe { job.assign(null_mut()) };
    assert!(
        !assign_invalid,
        "Assigning a null handle to Windows Job Object must return false"
    );

    // 2. ChildIsolationGuard::try_new succeeds for legitimate process
    let mut cmd = Command::new("cmd");
    cmd.args(["/C", "echo", "test_isolation"]);
    let child = cmd.spawn().expect("Spawn test child");

    let guard_res: Result<ChildIsolationGuard, _> = ChildIsolationGuard::try_new(&child);
    assert!(
        guard_res.is_ok(),
        "ChildIsolationGuard::try_new must return Ok for valid child process"
    );
    let guard = guard_res.unwrap();
    assert!(guard.job.is_some(), "Windows guard must hold Job Object");
}
