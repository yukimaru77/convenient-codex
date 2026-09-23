#!/usr/bin/env python3
"""Compare this fork's official base with the latest stable OpenAI Codex release."""

import argparse
from http.client import HTTPException
import json
from pathlib import Path
import re
import sys
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen


LATEST_RELEASE_API = "https://api.github.com/repos/openai/codex/releases/latest"
DEFAULT_MANIFEST = Path(__file__).resolve().parent.parent / "CONVENIENT_CODEX.json"


def release_version(tag):
    match = re.fullmatch(r"rust-v(\d+)\.(\d+)\.(\d+)", tag)
    if match is None:
        raise ValueError("expected a stable Codex release tag such as rust-v0.155.1")
    return tuple(int(part) for part in match.groups())


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--manifest",
        type=Path,
        default=DEFAULT_MANIFEST,
        help="product manifest (defaults to CONVENIENT_CODEX.json beside this checkout)",
    )
    args = parser.parse_args(argv)

    try:
        with args.manifest.open(encoding="utf-8") as stream:
            manifest = json.load(stream)
        upstream = manifest["upstream"]
        if upstream["repository"] != "openai/codex":
            raise ValueError("manifest upstream.repository must be openai/codex")
        current_tag = upstream["tag"]
        current_version = release_version(current_tag)

        request = Request(
            LATEST_RELEASE_API,
            headers={
                "Accept": "application/vnd.github+json",
                "User-Agent": "convenient-codex-upstream-check/1",
                "X-GitHub-Api-Version": "2022-11-28",
            },
        )
        with urlopen(request, timeout=20) as response:
            latest = json.load(response)
        if latest["draft"] is not False or latest["prerelease"] is not False:
            raise ValueError("GitHub latest release is not a published stable release")
        latest_tag = latest["tag_name"]
        latest_version = release_version(latest_tag)
    except HTTPError as error:
        print(
            "Upstream check failed: GitHub API returned HTTP "
            f"{error.code}; check network access or the API rate limit.",
            file=sys.stderr,
        )
        return 1
    except (OSError, URLError, HTTPException, ValueError, KeyError, TypeError) as error:
        print(f"Upstream check failed: {error}", file=sys.stderr)
        return 1

    if current_version == latest_version:
        status = "current base matches the latest stable release"
    elif current_version < latest_version:
        status = "newer stable release available; migration and validation required"
    else:
        status = "current base is newer than the latest release returned by GitHub"

    print(f"Current base:  {current_tag}")
    print(f"Latest stable: {latest_tag}")
    print(f"Status:        {status}")
    print(f"Release URL:   https://github.com/openai/codex/releases/tag/{latest_tag}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
