#!/usr/bin/env python3
"""Open an owned direct Kitty preview from any terminal, including tmux; no persistent config."""
import argparse
import os
from pathlib import Path
import subprocess
import sys
import tempfile

KITTY = Path("/Applications/kitty.app/Contents/MacOS/kitty")


def launch_configuration(directory, seconds, page="scripts"):
    directory = Path(directory)
    env = {key: os.environ[key] for key in ("PATH", "LANG", "LC_ALL", "TMPDIR") if key in os.environ}
    env.update(
        KITTY_CONFIG_DIRECTORY=str(directory),
        KITTY_CACHE_DIRECTORY=str(directory / "cache"),
        KITTY_RUNTIME_DIRECTORY=str(directory / "run"),
    )
    args = [str(KITTY), "--config", "NONE", "--title", "Plexmaton native text experiment"]
    for setting in (
        "allow_remote_control=no", "remember_window_size=no", "initial_window_width=88c",
        "macos_quit_when_last_window_closed=yes",
        f"initial_window_height={44 if page in ('ml', 'reply') else 36}c", "font_family=Menlo", "font_size=15",
        "background=#11131c", "foreground=#dce1ea", "window_padding_width=8",
        "shell_integration=disabled",
    ):
        args.extend(["--override", setting])
    args.extend([sys.executable, str(Path(__file__).with_name("preview.py")), "--seconds", str(seconds), "--page", page])
    return args, env


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--seconds", type=int, default=300)
    parser.add_argument("--page", choices=("scripts", "ml", "reply"), default="scripts")
    parser.add_argument("--reply-directory", type=Path)
    args = parser.parse_args()
    if sys.platform != "darwin" or not KITTY.is_file():
        parser.error("this launcher requires Kitty in /Applications on macOS")
    if not 1 <= args.seconds <= 600:
        parser.error("--seconds must be in 1..600")
    with tempfile.TemporaryDirectory(prefix="plexmaton-kitty-preview-") as directory:
        command, env = launch_configuration(directory, args.seconds, args.page)
        if args.reply_directory is not None:
            command.extend(["--reply-directory", str(args.reply_directory.resolve())])
        with subprocess.Popen(command, env=env, stdin=subprocess.DEVNULL) as child:
            try:
                return child.wait(timeout=args.seconds + 10)
            except (KeyboardInterrupt, subprocess.TimeoutExpired):
                child.terminate()
                try:
                    child.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait(timeout=3)
                return 1


if __name__ == "__main__":
    raise SystemExit(main())
