# Contributing to isi_music

Thanks for wanting to contribute. Here's everything you need to know.

---

## Getting started

1. Fork the repository and clone your fork
2. Install dependencies (see [README](README.md))
3. Create a branch from `master`:
   ```bash
   git checkout -b feat/my-feature
   ```
4. Make your changes, commit, and open a Pull Request

---

## Commit messages — Conventional Commits

All commits **must** follow the [Conventional Commits](https://www.conventionalcommits.org/) spec. This keeps the changelog clean and makes releases predictable.

### Format

```
<type>(<scope>): <short description>

[optional body]

[optional footer]
```

### Types

| Type | When to use |
|------|-------------|
| `feat` | New feature |
| `fix` | Bug fix |
| `perf` | Performance improvement |
| `refactor` | Code change that is neither a fix nor a feature |
| `style` | Formatting, whitespace — no logic change |
| `test` | Adding or fixing tests |
| `docs` | Documentation only |
| `chore` | Build process, dependency updates, tooling |
| `revert` | Reverts a previous commit |

### Examples

```
feat(player): skip unavailable tracks automatically
fix(ui): album art not re-rendering on resize
perf: replace custom allocator with libc
docs: add Windows install instructions
chore: bump ratatui to 0.30
```

### Rules

- Use the **imperative mood** in the description ("add", "fix", "remove" — not "added", "fixes")
- Keep the first line under **72 characters**
- Don't end the description with a period
- Reference issues in the footer: `Closes #42`
- Breaking changes go in the footer: `BREAKING CHANGE: config format changed`

---

## Branch naming

```
feat/short-description
fix/short-description
perf/short-description
refactor/short-description
docs/short-description
```

---

## Pull Requests

- One concern per PR — don't mix unrelated fixes
- Make sure `cargo build --release` passes before opening
- Write a clear description of **what** changed and **why**
- Link the related issue if there is one

---

---

## Reporting issues

Open an issue with:
- What you expected to happen
- What actually happened
- Steps to reproduce
- OS and terminal emulator
