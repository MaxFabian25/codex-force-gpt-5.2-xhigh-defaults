# Codex CLI — `gpt-5.2` defaults fork

This is an **unofficial fork** of OpenAI's Codex CLI (`openai/codex`).

The goal of this fork is simple: **make Codex default to `gpt-5.2` (not `gpt-5.2-codex`) everywhere**, with:

- `model_reasoning_effort = xhigh`
- `model_verbosity = high`
- `model_reasoning_summary = detailed`

This applies to:

- The main/default agent
- Spawned agents (orchestrator, worker, **explorer**)
- Built-in collaboration modes (Plan / Code / Pair Programming / Execute)

## What's different from upstream?

Compared to `openai/codex`, this fork intentionally makes only a small set of opinionated default changes:

- **Default model preset** is `gpt-5.2` (and defaults to `reasoning_effort = xhigh`).
- **Sub-agent model overrides** (orchestrator/worker/explorer) are pinned to `gpt-5.2` with `xhigh/high/detailed`.
- **Collaboration mode presets** are pinned to `gpt-5.2` with `reasoning_effort = xhigh`.
- **Default agent thread limit** is bumped to `8` (`DEFAULT_AGENT_MAX_THREADS = Some(8)`).

Nothing prevents you from selecting other models (including `gpt-5.2-codex`) via configuration or the model picker; the point here is purely to change the *defaults*.

## Using this fork

If you want the **official** Codex CLI distribution, install it from npm or Homebrew as usual (see below).

If you specifically want the **fork behavior**, build and run from source:

```shell
git clone https://github.com/MaxFabian25/codex-gpt-5.2-defaults.git
cd codex-gpt-5.2-defaults/codex-rs
cargo build -p codex-cli --release
./target/release/codex
```

---

<p align="center"><code>npm i -g @openai/codex</code><br />or <code>brew install --cask codex</code></p>
<p align="center"><strong>Codex CLI</strong> is a coding agent from OpenAI that runs locally on your computer.
<p align="center">
  <img src="./.github/codex-cli-splash.png" alt="Codex CLI splash" width="80%" />
</p>
</br>
If you want Codex in your code editor (VS Code, Cursor, Windsurf), <a href="https://developers.openai.com/codex/ide">install in your IDE.</a>
</br>If you are looking for the <em>cloud-based agent</em> from OpenAI, <strong>Codex Web</strong>, go to <a href="https://chatgpt.com/codex">chatgpt.com/codex</a>.</p>

---

## Quickstart

### Installing and running Codex CLI

Install globally with your preferred package manager:

```shell
# Install using npm
npm install -g @openai/codex
```

```shell
# Install using Homebrew
brew install --cask codex
```

Then simply run `codex` to get started.

<details>
<summary>You can also go to the <a href="https://github.com/openai/codex/releases/latest">latest GitHub Release</a> and download the appropriate binary for your platform.</summary>

Each GitHub Release contains many executables, but in practice, you likely want one of these:

- macOS
  - Apple Silicon/arm64: `codex-aarch64-apple-darwin.tar.gz`
  - x86_64 (older Mac hardware): `codex-x86_64-apple-darwin.tar.gz`
- Linux
  - x86_64: `codex-x86_64-unknown-linux-musl.tar.gz`
  - arm64: `codex-aarch64-unknown-linux-musl.tar.gz`

Each archive contains a single entry with the platform baked into the name (e.g., `codex-x86_64-unknown-linux-musl`), so you likely want to rename it to `codex` after extracting it.

</details>

### Using Codex with your ChatGPT plan

Run `codex` and select **Sign in with ChatGPT**. We recommend signing into your ChatGPT account to use Codex as part of your Plus, Pro, Team, Edu, or Enterprise plan. [Learn more about what's included in your ChatGPT plan](https://help.openai.com/en/articles/11369540-codex-in-chatgpt).

You can also use Codex with an API key, but this requires [additional setup](https://developers.openai.com/codex/auth#sign-in-with-an-api-key).

## Docs

- [**Codex Documentation**](https://developers.openai.com/codex)
- [**Contributing**](./docs/contributing.md)
- [**Installing & building**](./docs/install.md)
- [**Open source fund**](./docs/open-source-fund.md)

This repository is licensed under the [Apache-2.0 License](LICENSE).
