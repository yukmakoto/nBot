#!/usr/bin/env python3
from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path


def eprint(*args: object) -> None:
    print(*args, file=sys.stderr)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="SSH into a server and run nBot docker diagnostics (non-interactive)."
    )
    parser.add_argument("--host", required=True, help="SSH host/IP")
    parser.add_argument("--port", type=int, default=22, help="SSH port (default: 22)")
    parser.add_argument("--user", default="root", help="SSH username (default: root)")

    auth = parser.add_argument_group("auth")
    auth.add_argument(
        "--password",
        default=None,
        help="SSH password (NOT recommended; prefer env var or key).",
    )
    auth.add_argument(
        "--password-env",
        default="NBOT_SSH_PASSWORD",
        help="Env var name to read the SSH password from (default: NBOT_SSH_PASSWORD).",
    )
    auth.add_argument(
        "--key",
        default=None,
        help="SSH private key path (recommended).",
    )

    parser.add_argument(
        "--nbot-dir",
        default="/opt/nbot",
        help="nBot install dir on remote host (default: /opt/nbot).",
    )
    parser.add_argument(
        "--remote-path",
        default="/tmp/nbot-diagnose.sh",
        help="Remote temp path to upload the script (default: /tmp/nbot-diagnose.sh).",
    )
    parser.add_argument(
        "--connect-timeout",
        type=int,
        default=15,
        help="SSH connect timeout in seconds (default: 15).",
    )
    parser.add_argument(
        "--command-timeout",
        type=int,
        default=180,
        help="Remote command timeout in seconds (default: 180).",
    )
    return parser.parse_args()


def load_local_script() -> str:
    script_path = Path(__file__).with_name("diagnose.sh")
    if not script_path.exists():
        raise FileNotFoundError(f"Missing local script: {script_path}")
    return script_path.read_text(encoding="utf-8")


def main() -> int:
    args = parse_args()

    try:
        import paramiko  # type: ignore
    except Exception:
        eprint("Missing dependency: paramiko")
        eprint("Install: python3 -m pip install --user paramiko")
        return 2

    password = args.password
    if password is None:
        password = os.environ.get(args.password_env)

    if not password and not args.key:
        eprint("No auth provided.")
        eprint(
            f"Provide --key /path/to/id_rsa OR set {args.password_env}=... OR pass --password ..."
        )
        return 2

    try:
        script = load_local_script()
    except Exception as exc:
        eprint(str(exc))
        return 2

    ssh = paramiko.SSHClient()
    # Non-interactive convenience. If you want strict verification, pre-populate known_hosts instead.
    ssh.set_missing_host_key_policy(paramiko.AutoAddPolicy())

    try:
        ssh.connect(
            hostname=args.host,
            port=args.port,
            username=args.user,
            password=password,
            key_filename=args.key,
            timeout=args.connect_timeout,
            banner_timeout=args.connect_timeout,
            auth_timeout=args.connect_timeout,
            look_for_keys=False,
            allow_agent=False,
        )
    except Exception as exc:
        eprint(f"SSH connect failed: {exc}")
        return 1

    remote_path: str = args.remote_path
    try:
        sftp = ssh.open_sftp()
        with sftp.file(remote_path, "w") as f:
            f.write(script)
        sftp.close()

        # Ensure executable.
        ssh.exec_command(f"chmod +x {remote_path!s}", timeout=args.command_timeout)

        # Run it with NBOT_DIR override.
        cmd = f"NBOT_DIR='{args.nbot_dir}' bash {remote_path!s}"
        stdin, stdout, stderr = ssh.exec_command(cmd, timeout=args.command_timeout)
        _ = stdin  # unused
        out = stdout.read().decode("utf-8", errors="replace")
        err = stderr.read().decode("utf-8", errors="replace")
        if out:
            sys.stdout.write(out)
        if err:
            sys.stderr.write(err)

        return stdout.channel.recv_exit_status()
    except Exception as exc:
        eprint(f"Remote exec failed: {exc}")
        return 1
    finally:
        ssh.close()


if __name__ == "__main__":
    raise SystemExit(main())

