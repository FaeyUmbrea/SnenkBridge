# Contributing

Contributions to **SnenkBridge** are welcome.

This document describes the basic contribution process and the licensing terms that apply when contributing to the project.

## Making a contribution

1. Fork the repository.
2. Create a branch for your changes.
3. Make and test your changes.
4. Open a pull request against the repository's main development branch.

Follow the build instructions in [README.md](README.md) and the formatting, linting, and testing instructions below. You'll need Rust and Cargo with the repository's configured toolchain. Run `npm run check`, `npm run lint`, `npm test`, and `cargo test --workspace` to verify your changes.

## Pull requests

Pull requests should include a short description of:

- what was changed;
- why the change was made;
- any relevant implementation details;
- how the change was tested.

If a change affects documented behaviour, configuration, public APIs, or other user-facing functionality, update the relevant documentation as part of the same contribution where appropriate.

Try to keep pull requests focused on a single change or closely related set of changes. This makes changes easier to review and maintain.

## Commit messages

We use [Conventional Commits](https://www.conventionalcommits.org/) so our changelog can be generated automatically. The format is:

```
type: short description
```

Common types:

- **feat** - Adding new functionality
- **fix** - Fixing a bug
- **docs** - Changes to documentation only
- **refactor** - Code changes that don't add features or fix bugs
- **chore** - Maintenance stuff, CI changes, etc.

A few examples:

```
feat: add VBridger config import
fix: handle missing blend shapes gracefully
docs: clarify tracking client setup
refactor: simplify expression parser
chore: update dependencies
```

If your change is scoped to a specific area, you can add that in parentheses:

```
feat(ui): add config file picker
fix(tracking): reconnect on timeout
```

That's really all there is to it. Don't overthink the type - just pick whichever one feels right.

## Code style

- Run `cargo fmt` before committing (or set up your editor to do it on save)
- Make sure `cargo clippy --all-targets -- -D warnings`, `npm run check`, and `npm run lint` pass with zero warnings
- CI will check both of these automatically

## AI-assisted development

We reserve the right to reject any contribution that appears to be AI-generated, for any reason, without further discussion. This is not up for debate.

Human-written code is strongly preferred. If you use AI tools (Copilot, Claude, ChatGPT, etc.) as a development aid, that's your business, but **you are responsible for every line of code in your PR.** You need to be able to explain what it does and why it's there. "Vibe coded" contributions - where AI output gets submitted without genuine understanding - will be rejected.

If AI was involved in writing a significant portion of your contribution, say so in the PR description. This helps reviewers know what to look more closely at.

Do not add AI tools as co-authors in your commits (e.g. `Co-Authored-By: GitHub Copilot`, `Co-Authored-By: Claude`, etc.). The git history is not an advertisement space. PRs containing AI co-author tags will be rejected until the commit history is cleaned up.

## Third-party material

Only submit code or other material that you have the right to contribute under the terms described below.

If your contribution contains or is derived from third-party code or other copyrighted material, you must ensure that its license permits its inclusion in this project.

Please identify relevant third-party material and its license in the pull request when it is not already clearly documented in the source.

## Licensing

### License of contributions

By submitting a contribution to this repository, you agree to license your contribution under the same license as the project codebase, as specified in [LICENSE](LICENSE).

You retain copyright in your contribution. Contributing does not transfer ownership of your contribution to the project maintainers.

You represent that you have the right to make the contribution available under these terms.

### Steam and Valve exception

The project may be distributed through Valve Corporation's Steam platform and may be linked, combined, or otherwise integrated with software provided by Valve Corporation, including but not limited to the Steamworks SDK.

By submitting a contribution, you additionally agree that, solely with respect to your contribution as incorporated into **SnenkBridge**:

- Valve Corporation and software provided by Valve Corporation are exempt from obligations imposed by the project's license to the extent that those obligations would otherwise arise solely as a result of linking, combining, interacting with, or distributing the project together with Valve software; and
- the project may be compiled, linked or combined with Valve software and the resulting application may be distributed through the Steam platform.

This exception applies only to **SnenkBridge** as maintained by its canonical project repository.

For the purposes of this exception, the canonical project repository is this repository and any successor repository to which stewardship of **SnenkBridge** is transferred.

#### Scope of this exception

This exception is intentionally narrow.

It:

- does **not** change the license under which your contribution is otherwise made available;
- does **not** transfer copyright in your contribution;
- does **not** grant a general right to relicense your contribution;
- does **not** exempt the project's own GPL-covered code from the requirements of the GPL;
- does **not** grant unrelated projects permission to combine your contribution with Valve software outside the terms of its normal license;
- does **not** apply to forks or other projects merely because they contain or derive from your contribution; and
- exists only to permit the canonical project to integrate with Valve software and be distributed through Steam without imposing the project's license obligations on Valve Corporation or Valve software solely because of that integration or distribution.

Outside this exception, your contribution remains subject to the project's normal license terms.

### Acceptance of these terms

Submitting a pull request or other contribution to this repository constitutes acceptance of the contribution terms above.

If you do not agree to these terms, do not submit the contribution.
