#!/usr/bin/env node
const { spawn } = require("node:child_process");
const { arch, argv, exit, kill, pid, platform } = require("node:process");

const suffix = { darwin: "", linux: "-gnu", win32: "-msvc" }[platform];
if (suffix === undefined) {
  throw new Error(`Unsupported platform: ${platform}`);
}
const packageName = `asobou-${platform}-${arch}${suffix}`;
const binaryName = platform === "win32" ? "asobou.exe" : "asobou";
const binary = require.resolve(`${packageName}/${binaryName}`);
const child = spawn(binary, argv.slice(2), { stdio: "inherit" });
child.once("error", (error) => {
  throw error;
});
child.once("exit", (code, signal) => (signal ? kill(pid, signal) : exit(code ?? 1)));
