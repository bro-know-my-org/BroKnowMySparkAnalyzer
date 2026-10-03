# bkmsa-cli

Native command-line interface for [BroKnowMySparkAnalyzer](https://github.com/bro-know-my-org/BroKnowMySparkAnalyzer). The installed executable is named `bkmsa`.

```bash
cargo install bkmsa-cli
bkmsa --help
bkmsa inspect report.sparkprofile
```

AI analysis uses an OpenAI-compatible provider:

```bash
export BKMSA_API_KEY="..."
export BKMSA_BASE_URL="https://api.openai.com/v1"
export BKMSA_MODEL="gpt-4.1-mini"

bkmsa analyze report.sparkprofile
```

See the [CLI guide](https://github.com/bro-know-my-org/BroKnowMySparkAnalyzer/blob/master/docs/CLI.md) for configuration, report tools, output formats, and stdin. The [repository README](https://github.com/bro-know-my-org/BroKnowMySparkAnalyzer/blob/master/README.md) covers supported report types, Web/desktop usage, and release downloads. Run `bkmsa --help` or `bkmsa <command> --help` for command options.
