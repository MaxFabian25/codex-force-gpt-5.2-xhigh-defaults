<p align="center"><code>git clone https://github.com/MaxFabian25/codex-force-gpt-5.4-xhigh-defaults.git</code><br /><code>cd codex-force-gpt-5.4-xhigh-defaults/codex-rs && cargo run --bin codex</code></p>
<p align="center"><strong>Codex CLI</strong> is a maintained fork of the OpenAI Codex CLI that runs locally on your computer.
<p align="center">
  <img src="./.github/codex-cli-splash.png" alt="Codex CLI splash" width="80%" />
</p>
</br>
If you want Codex in your code editor (VS Code, Cursor, Windsurf), <a href="https://developers.openai.com/codex/ide">install in your IDE.</a>
</br>If you want the desktop app experience, run <code>codex app</code> or visit <a href="https://chatgpt.com/codex?app-landing-page=true">the Codex App page</a>.
</br>If you are looking for the <em>cloud-based agent</em> from OpenAI, <strong>Codex Web</strong>, go to <a href="https://chatgpt.com/codex">chatgpt.com/codex</a>.</p>

---

## Fork defaults

This fork tracks upstream stable `rust-v0.111.0` and hard-overrides the default model stack to `gpt-5.4` with reasoning `xhigh`, verbosity `high`, and reasoning summaries `detailed`.

## Distribution model

This repository publishes fork-specific GitHub releases for tagged fork states. It does not publish a separate npm package or Homebrew cask.

Installing `@openai/codex`, `brew install --cask codex`, or downloading upstream OpenAI releases gives you the upstream CLI and upstream defaults, not this fork.

Use this fork when you want the maintained `gpt-5.4` default override, the fork-specific release notes, or the exact source state behind the published fork tags.

## Quickstart

### Building and running this fork

Clone the fork and run the Rust CLI directly from source:

```shell
git clone https://github.com/MaxFabian25/codex-force-gpt-5.4-xhigh-defaults.git
cd codex-force-gpt-5.4-xhigh-defaults/codex-rs
cargo run --bin codex
```

That path keeps the fork separate from any upstream npm/Homebrew install you already use.

If you want the latest tagged fork state, use the [GitHub Releases](https://github.com/MaxFabian25/codex-force-gpt-5.4-xhigh-defaults/releases). Releases currently provide release notes plus GitHub's generated source archives; source-build remains the supported way to run the forked CLI.

For full system requirements and local build helpers, see [Installing & building](./docs/install.md).

### Using Codex with your ChatGPT plan

Run `codex` and select **Sign in with ChatGPT**. We recommend signing into your ChatGPT account to use Codex as part of your Plus, Pro, Team, Edu, or Enterprise plan. [Learn more about what's included in your ChatGPT plan](https://help.openai.com/en/articles/11369540-codex-in-chatgpt).

You can also use Codex with an API key, but this requires [additional setup](https://developers.openai.com/codex/auth#sign-in-with-an-api-key).

## Docs

- [**Codex Documentation**](https://developers.openai.com/codex)
- [**Contributing**](./docs/contributing.md)
- [**Installing & building**](./docs/install.md)
- [**Open source fund**](./docs/open-source-fund.md)

This repository is licensed under the [Apache-2.0 License](LICENSE).
