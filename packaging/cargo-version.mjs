#!/usr/bin/env bun

import { readFileSync, writeFileSync } from "node:fs";

function usage() {
  console.error("Usage: bun packaging/cargo-version.mjs set <version> [path]");
  process.exit(1);
}

function setPackageVersion(path, version) {
  const raw = readFileSync(path, "utf8");
  const lines = raw.split(/\r?\n/);
  let inPackage = false;
  let updated = false;

  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i];
    if (/^\[package\]\s*$/.test(line)) {
      inPackage = true;
      continue;
    }
    if (inPackage && /^\[/.test(line)) {
      break;
    }
    if (inPackage && /^version\s*=\s*".*"$/.test(line)) {
      lines[i] = `version = "${version}"`;
      updated = true;
      break;
    }
  }

  if (!updated) {
    throw new Error("Could not find version in [package] section");
  }

  const newline = raw.includes("\r\n") ? "\r\n" : "\n";
  const hadTrailingNewline = raw.endsWith(newline);
  const next = lines.join(newline) + (hadTrailingNewline ? newline : "");
  writeFileSync(path, next, "utf8");
}

const [, , cmd, version, path = "Cargo.toml"] = process.argv;

if (cmd !== "set" || !version) {
  usage();
}

if (!/^\d+\.\d+\.\d+([-.][0-9A-Za-z.-]+)?$/.test(version)) {
  throw new Error(`Invalid version: ${version}`);
}

setPackageVersion(path, version);
console.log(`Updated ${path} [package].version to ${version}`);
