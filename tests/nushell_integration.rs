//! Nushell hook: apply on enter, unset on leave, re-apply on re-enter.
//!
//! Nushell has no signal trap, so the hook polls the env file at
//! pre_prompt/pre_execution. This exercises the synchronous no-mux path
//! (which requires the JSON-emitting nushell mode in `start`).

mod common;
use common::*;
use std::fs;
use std::process::Command;

#[test]
fn nushell_apply_unset_reenter() {
    require!("nu");
    let sb = Sandbox::new("export NU_TEST_VAR=hello_from_direnv\n").unwrap();

    // Materialize the hook from the binary so the test doesn't depend on
    // the source tree's hooks/ dir being present.
    let hook_out = Command::new(bin()).args(["hook", "nu"]).output().unwrap();
    assert!(hook_out.status.success());
    let hook_path = sb.dir.join("nushell.nu");
    fs::write(&hook_path, &hook_out.stdout).unwrap();

    let bin_dir = bin().parent().unwrap().to_path_buf();
    let script = sb.dir.join("drive.nu");
    fs::write(
        &script,
        format!(
            r#"$env.PATH = ([
    "{bin_dir}"
    (which direnv | get path | first | path dirname)
] | append ($env.PATH | split row (char esep)))
source {hook}

cd {dir}
_direnv_instant_hook
print $"enter=($env.NU_TEST_VAR? | default '<unset>')"

cd /
_direnv_instant_hook
print $"leave=($env.NU_TEST_VAR? | default '<unset>')"

cd {dir}
_direnv_instant_hook
print $"reenter=($env.NU_TEST_VAR? | default '<unset>')"
"#,
            bin_dir = bin_dir.display(),
            hook = hook_path.display(),
            dir = sb.dir.display()
        ),
    )
    .unwrap();

    let env = sb.base_env();
    let out = Command::new("nu")
        .arg(&script)
        .env_clear()
        .envs(&env)
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "nu failed\n--stdout--\n{stdout}\n--stderr--\n{stderr}"
    );
    assert!(stdout.contains("enter=hello_from_direnv"), "{stdout}");
    assert!(stdout.contains("leave=<unset>"), "{stdout}");
    assert!(stdout.contains("reenter=hello_from_direnv"), "{stdout}");
}
