# bkmsa-agent

`bkmsa-agent` runs the evidence-driven AI analysis loop shared by the BroKnowMySparkAnalyzer CLI, desktop applications, and WebAssembly backend.

Enable the `native-client` feature to use the built-in OpenAI-compatible HTTP client. Without it, hosts can provide their own `ChatClient` implementation.
