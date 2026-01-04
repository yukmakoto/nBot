use std::io::ErrorKind;
use std::path::Path;
use tokio::process::Command;

fn resolve_program(program: &str) -> String {
    if program == "ffmpeg" {
        crate::tool::ffmpeg_program()
    } else if program == "ffprobe" {
        crate::tool::ffprobe_program()
    } else {
        program.to_string()
    }
}

async fn run_program_local(
    program: &str,
    work_dir: &Path,
    args: &[&str],
) -> Result<std::process::Output, std::io::Error> {
    let program = resolve_program(program);
    Command::new(program)
        .current_dir(work_dir)
        .args(args)
        .output()
        .await
}

pub(super) async fn run_program(
    program: &str,
    work_dir: &Path,
    args: &[&str],
) -> Result<std::process::Output, String> {
    run_program_local(program, work_dir, args)
        .await
        .map_err(|e| {
            if e.kind() == ErrorKind::NotFound {
                format!("{program} 不存在：请安装 ffmpeg")
            } else {
                format!("{program} failed: {e}")
            }
        })
}
