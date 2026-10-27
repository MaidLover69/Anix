# Security Policy

## Supported versions

Only the latest release gets security fixes. Update first and check whether the problem is still there.

| Version | Supported |
| :--- | :--- |
| 1.3.x (latest) | ✅ |
| 1.2.x and older | ❌ |

## Reporting a vulnerability

**Please do not open a public issue for a security problem.**

Report it privately with GitHub's [Report a vulnerability](https://github.com/MaidLover69/Anix/security/advisories/new)
form (repository › **Security** › **Report a vulnerability**). Only the maintainer can see it.

Please include:

- the Anix version and how you installed it (AppImage, RPM, DEB or built from source);
- your distribution and desktop;
- what an attacker can do, and the steps or a small proof of concept that shows it;
- whether it needs a malicious extension, a malicious website, or access to the computer.

Do not include real account tokens, passwords or anyone's personal data. If logs are useful, use
**Settings › System › Diagnostics report**, which removes logins, tokens, email addresses and your home folder.

## What happens next

This is a one-person project, so these are aims, not guarantees:

- you get a reply within **7 days**;
- once the problem is confirmed, a fix ships in the next release, as fast as its severity needs;
- you are credited in the changelog, unless you would rather not be.

Please give the fix time to ship before you publish details.

## Scope

**In scope:** the Anix app in this repository, for example:

- the local stream proxy (`src-tauri/src/stream_proxy.rs`): access without its token, or use as an open proxy;
- the extension sandbox: an extension reading files, starting programs, or reaching private or loopback addresses;
- account tokens leaking to the page, logs, diagnostics reports or other files;
- deep links (`anix://`), external players and file paths that run or open something they should not;
- the update check accepting a file that does not match its checksum.

**Not in scope:**

- bugs in a third-party extension: report them to that extension's author. Problems in the
  extensions maintained for Anix go to the [extensions repository](https://github.com/MaidLover69/Anix_plugins/issues).
- the websites that extensions read from;
- problems that need someone to already control your user account or computer;
- requests to get around a website's anti-bot checks. Anix will not add that.
