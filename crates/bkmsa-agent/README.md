# bkmsa-agent

`bkmsa-agent` runs the evidence-driven AI analysis loop shared by the BroKnowMySparkAnalyzer CLI, desktop applications, and WebAssembly backend.

Enable the `native-client` feature to use the built-in OpenAI-compatible HTTP client. Without it, hosts can provide their own `ChatClient` implementation.

Initial diagnoses and follow-up answers describe sampling goals, modes, durations, and comparison metrics without generating concrete Spark commands. Shared output validation rejects command-shaped text (including code, console forms, aliases, and namespaces) and asks the model to rewrite it; exhausted retries return a fixed fallback. Rejected assistant text is also hidden from analysis traces. Exact syntax should come from the installed build's official help, since reports do not reliably identify command compatibility or capabilities.
