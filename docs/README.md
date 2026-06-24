# filetree documentation

Reference documentation for **filetree-mac**. Start at the repo root [agent.md](../agent.md) for the agent index and doc-sync rules.

## Contents

| File | Description |
|------|-------------|
| [architecture.md](architecture.md) | High-level design and scan/TUI data flow |
| [modules.md](modules.md) | Source module reference (`src/*.rs`) |
| [development.md](development.md) | Local setup, testing, linting, coverage |
| [features.md](features.md) | User features and keyboard shortcuts |
| [security.md](security.md) | Path safety, install script, export policy |
| [changelog.md](changelog.md) | Change history |

## Conventions

- **User docs** — [README.md](../README.md) stays short: install, quick start, FDA, shortcuts summary.
- **Agent docs** — This folder holds detail agents need to change code safely.
- **Sync rule** — Any code change (feature, fix, improvement) must update [changelog.md](changelog.md) and any other affected doc. See [agent.md](../agent.md#mandatory-rule-keep-docs-in-sync).

## Adding new docs

1. Create the file under `docs/`.
2. Add a row to the table above.
3. Add a row to the index in [agent.md](../agent.md#documentation-index).
4. Add a `### Added` entry in [changelog.md](changelog.md).