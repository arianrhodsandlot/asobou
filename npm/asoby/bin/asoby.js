#!/usr/bin/env node

const { spawn } = require("node:child_process");
const { dirname, join } = require("node:path");

const packages = {
  "darwin-x64": "asoby-darwin-x64",
  "darwin-arm64": "asoby-darwin-arm64",
  "linux-x64": "asoby-linux-x64-gnu",
  "linux-arm64": "asoby-linux-arm64-gnu",
  "win32-x64": "asoby-win32-x64-msvc",
  "win32-arm64": "asoby-win32-arm64-msvc",
};

const platform = `${process.platform}-${process.arch}`;
const packageName = packages[platform];

if (!packageName) {
  console.error(`asoby does not provide a binary for ${process.platform}/${process.arch}.`);
  process.exit(1);
}

let packageJson;
try {
  packageJson = require.resolve(`${packageName}/package.json`);
} catch {
  console.error(`The optional package ${packageName} is not installed.`);
  console.error("Try reinstalling asoby for this platform.");
  process.exit(1);
}

const binaryName = process.platform === "win32" ? "asoby.exe" : "asoby";
const binary = join(dirname(packageJson), binaryName);
const child = spawn(binary, process.argv.slice(2), { stdio: "inherit" });

child.once("error", (error) => {
  console.error(`Failed to start asoby: ${error.message}`);
  process.exit(1);
});

child.once("exit", (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
    return;
  }

  process.exit(code ?? 1);
});
