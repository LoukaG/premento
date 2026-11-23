# PRemento — PR review copilot

A native Rust desktop app (egui) that lists the open GitHub pull requests where you are
**assignee** or **reviewer** (review requested, or already reviewed). It runs an AI review of any
PR with the LLM CLI you already have on your machine: Claude Code, Codex, Gemini CLI, Ollama, or
any other command.

Each analysis has three parts:

| Tab | What you get |
|---|---|
| **Risk-Based Review** | Risk score computed from complexity, worst case and audience (see [Risk score](#risk-score)), how deep to review, and risk factors with severity, files and what to verify |
| **Intent & Decision Log** | What the PR says it does and what the code actually does, whether they match, the design decisions taken (why, alternatives, source), and questions to ask the author |
| **Service Map** (attraction-inc PRs) | The attraction-inc architecture as a map, with each impacted service highlighted: **direct** (its code or config changes) or **indirect** (affected through a connection), colored by severity, with the reason |
| **Impact Analysis** (other repos) | Affected components, breaking changes, downstream effects, data/migrations, security, performance, test coverage and suggested tests |

The **Overview** tab sums these up.

The **Ask** tab runs Claude Code or Codex in a terminal built into the tab, in a folder holding the
PR (description, commits, discussion and diff, fetched when you start) and its review, so you can ask
questions about it. It uses the tool and model picked above the tabs. Click the terminal to type;
drag to select and Cmd+C to copy, Cmd+V to paste, the mouse wheel scrolls back. Sessions keep running
when you switch tabs or PRs; **End** stops one. **Open in Terminal app** runs the same session in a
separate terminal instead. The folder is kept per PR (`~/Library/Caches/memento/ask/<pr>`), so
`claude --continue` or `codex resume --last` there picks up the last conversation. Codex runs in a
read-only sandbox. The first time, Claude Code asks whether to trust the folder.

## Requirements

- Rust (edition 2024)
- [`gh`](https://cli.github.com/), logged in (`gh auth login`). PRemento uses it for every GitHub call, so it needs no token setup of its own.
- At least one LLM CLI on your `PATH` (`claude`, `codex`, `gemini`, `ollama`, …)

## Run

```bash
cargo run --release
```

- **⟳ Refresh** reloads the PR list. Filter it with *All / Assigned / Reviewer* or the search box.
- To analyze a PR, select it, choose a tool and a model (e.g. Claude Code with `fable`, `opus`, `sonnet` or `haiku`; **Other…** to type any model name) and click **🔍 Analyze**. Analyses run in the background, so you can start several at once, and **Cancel** stops one.
- Results are cached on disk and appear again on the next launch. The list shows each PR's risk badge.

Headless mode prints the analysis as JSON:

```bash
cargo run --release -- analyze owner/repo#123 "Claude Code" haiku   # model is optional
```

## Configuration

On first run, a config file is written to `~/Library/Application Support/memento/config.toml` (macOS) or `~/.config/memento/config.toml` (Linux):

PRemento retains the original `memento` settings and cache directories so existing configurations and reviews remain available.

```toml
default_provider = "Claude Code"
max_diff_chars = 150000   # the diff is truncated beyond this
timeout_secs = 900
search_limit = 100

[[providers]]
name = "Claude Code"
command = "claude"
args = ["-p", "--output-format", "text"]
stdin = true
model_args = ["--model", "{model}"]          # how the chosen model is passed
models = ["fable", "opus", "sonnet", "haiku"] # offered in the Model picker

[[providers]]
name = "Codex"
command = "codex"
args = ["exec", "{model_args}", "--skip-git-repo-check", "--sandbox", "read-only", "--ephemeral", "-o", "{output_file}", "-"]
stdin = true
model_args = ["-m", "{model}"]
```

The prompt is sent to the provider on stdin. Three placeholders can appear in `args`:
`{prompt_file}` (a file containing the prompt), `{output_file}` (when present, the answer is
read from this file instead of stdout) and `{model_args}` (where the model args go; without it they
are put first). Add a `[[providers]]` block for any other tool.

**Ask tab.** `chat_args` are the args for an interactive session, with `{prompt}` (the opening
message) and `{model_args}`. Claude Code and Codex have built-in defaults; set `chat_args = []` to hide
a tool from the Ask tab. The terminal opened is Terminal.app on macOS and `x-terminal-emulator` on
Linux; set e.g. `terminal = ["open", "-a", "iTerm", "{script}"]` to use another one.

**Models.** A provider with `model_args` gets a Model picker: **Default** (the tool's own default,
or `default_model` if set), each entry of `models`, or **Other…** to type any model name. When
Default is picked and there is no `default_model`, no model args are passed. The model used is shown
with each analysis (e.g. `Claude Code · haiku`). Config files written by earlier versions get these
options automatically, as long as the provider's `args` were not edited.

## Risk score

The LLM rates three dimensions, and PRemento computes the score from them (`RiskReview::compute` in `src/features/analysis/scoring.rs`):

| Dimension | Values |
|---|---|
| **Complexity** | 0-10: how hard the change is to get right |
| **Worst case** | `cosmetic` (1), `feature` (3): one feature stops working, e.g. a preview renders wrong, `core_flow` (7): checkout, payments, orders, data sync, `system_wide` (10): login/auth, whole app crashes, data loss, security |
| **Audience** | `customer` (×1.0), `partner` (×0.85), `internal` admin/staff (×0.6), `developer` CI/tooling (×0.3) |

`score = worst_case × audience × (0.6 + 0.4 × complexity / 10)`, then the level is: low < 3.5 ≤ medium < 6 ≤ high < 8 ≤ critical.

The worst case sets the ceiling and complexity only moves the score within it, so a change that can
only break one feature (max 3.0) is always low, however complex. A one-line change to customer login
is 6.8 (high), and a moderately complex one is critical. Cached analyses are re-scored on load;
ones from before the three dimensions existed keep the score the LLM gave them.

## What is sent to the LLM

The PR title, description, labels, branches, commits, changed files, the conversation (comments, reviews and inline review comments) and the diff. The LLM runs in a temporary empty directory.

## Service map

The map of attraction-inc services and their main connections is defined in `src/features/service_map/graph.rs`. It is based on `attraction-inc/workspace` (`ARCHITECTURE.md`, `docker-compose.yml`). When the PR is in attraction-inc, the map is sent to the LLM, which returns `service_impact`. The PR's own repo is always shown as directly impacted. PRs without an AI result use an estimate: the repo's service plus the services that call it.

The full map is shown when no PR is selected, and every attraction-inc PR in the list shows the services it impacts. Update `NODES`/`EDGES` in `src/features/service_map/graph.rs` when the architecture changes.

## Layout

Code is grouped by feature. Each feature folder holds its data, logic, state and views. Shared code lives in `core/` and `ui/`.

```
src/
  main.rs                  entry point
  cli.rs                   headless `analyze` command
  app/                     app shell: holds each feature's state, routes background events, lays out the panels
    mod.rs  messages.rs  top_bar.rs  details.rs (tabs)
    snapshots.rs           offscreen UI snapshots + glyph check (tests), with demo_analysis.json
  core/                    config.rs (settings), paths.rs (config/cache dirs), format.rs (relative times)
  ui/                      design system: theme.rs (palette, style), widgets.rs (pills, cards, tabs),
                           blocks.rs (stat tiles, list cards, callouts, banners), icon.rs
  features/
    pull_requests/         model.rs, github.rs (gh CLI: PR search + context), state.rs (list, filter,
                           selection), list_view.rs (sidebar), header.rs
    analysis/              model.rs, scoring.rs (risk score), prompt.rs (prompt, schema, parsing),
                           cache.rs, state.rs (running jobs + results), views/ (action bar + one file per tab)
    ask/                   session.rs (prepares the PR folder + command), state.rs, view.rs (Ask tab)
    providers/             config.rs (provider settings + defaults), runner.rs (runs a CLI with
                           timeout/cancel), picker.rs (tool + model pickers)
    service_map/           graph.rs (services + connections), impact.rs (impact + prompt), view.rs (map)
    terminal/              terminal emulator widget (alacritty_terminal + PTY): mod.rs, input.rs, paint.rs, palette.rs
```

## UI snapshots

The UI can be rendered offscreen with demo data (dark + light, every tab):

```bash
MEMENTO_SNAPSHOT_DIR=/tmp/memento-shots cargo test render_screens
```

A test (`all_ui_glyphs_exist`) also fails if the UI uses a character the bundled fonts cannot draw.
