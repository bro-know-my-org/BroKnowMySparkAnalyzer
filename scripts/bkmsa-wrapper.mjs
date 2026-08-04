import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";

export function runBkmsa(args) {
  const scriptDir = path.dirname(fileURLToPath(import.meta.url));
  const repository = path.resolve(scriptDir, "..");
  const configured = process.env.BKMSA_BIN?.trim();

  let command;
  let commandArgs;
  if (configured) {
    command = configured;
    commandArgs = args;
  } else {
    command = "cargo";
    commandArgs = [
      "run",
      "--quiet",
      "--manifest-path",
      path.join(repository, "Cargo.toml"),
      "-p",
      "bkmsa-cli",
      "--",
      ...args,
    ];
  }

  const result = spawnSync(command, commandArgs, { stdio: "inherit" });
  if (result.error) {
    console.error(`unable to start bkmsa: ${result.error.message}`);
    return 6;
  }
  return result.status ?? 6;
}
