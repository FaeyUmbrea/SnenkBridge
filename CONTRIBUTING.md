# Contributing to SnenkBridge

Thanks for wanting to help out! Here's what you need to know.

## Getting Started

1. Fork the repo and clone your fork
2. Make sure you have Rust installed (stable toolchain)
3. Run `cargo test --workspace` to verify everything builds
4. Create a branch for your changes

## Commit Messages

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

## Code Style

- Run `cargo fmt` before committing (or set up your editor to do it on save)
- Make sure `cargo clippy -- -D warnings` passes
- CI will check both of these automatically

## AI-Assisted Development

We reserve the right to reject any contribution that appears to be AI-generated, for any reason, without further discussion. This is not up for debate.

Human-written code is strongly preferred. If you use AI tools (Copilot, Claude, ChatGPT, etc.) as a development aid, that's your business, but **you are responsible for every line of code in your PR.** You need to be able to explain what it does and why it's there. "Vibe coded" contributions - where AI output gets submitted without genuine understanding - will be rejected.

If AI was involved in writing a significant portion of your contribution, say so in the PR description. This helps reviewers know what to look more closely at.

Do not add AI tools as co-authors in your commits (e.g. `Co-Authored-By: GitHub Copilot`, `Co-Authored-By: Claude`, etc.). The git history is not an advertisement space. PRs containing AI co-author tags will be rejected until the commit history is cleaned up.

## Pull Requests

- Keep PRs focused on one thing when possible
- Make sure tests pass before opening
- A short description of what and why is plenty for the PR body
