import { useState } from "react";
import type { IpcClient } from "../../ipc/client";
import { PROVIDER_IDS } from "../../ipc/contracts";
import type { AnvilConfig, AnvilStatus } from "../types";
import { asErrorMessage, parseAnvilConfig, parseAnvilStatus } from "../utils";
import type { ConsoleHook } from "./useConsole";

const DEFAULT_CONFIG: AnvilConfig = {
  autoStartOnOpen: true,
  forkUrl: "",
  port: 9545,
  chainId: 1,
};

const DEFAULT_STATUS: AnvilStatus = {
  running: false,
  port: null,
  url: null,
  projectPath: null,
  chainId: 1,
  account: null,
  accountIndex: 1,
  config: DEFAULT_CONFIG,
};

export interface AnvilHook {
  status: AnvilStatus;
  config: AnvilConfig;
  action: "start" | "stop" | "save" | null;
  setStatus: (status: AnvilStatus) => void;
  setConfig: (config: AnvilConfig) => void;
  loadStatus: (options?: { silent?: boolean }) => Promise<{ error?: string }>;
  loadConfig: (options?: { silent?: boolean }) => Promise<{ error?: string }>;
  saveConfig: (config: AnvilConfig) => Promise<{ error?: string; status?: string }>;
  start: (projectPath?: string, options?: { auto?: boolean }) => Promise<{ error?: string; status?: string }>;
  stop: () => Promise<{ error?: string; status?: string }>;
}

export function useAnvil(client: IpcClient, console_: ConsoleHook): AnvilHook {
  const [status, setStatus] = useState<AnvilStatus>(DEFAULT_STATUS);
  const [config, setConfig] = useState<AnvilConfig>(DEFAULT_CONFIG);
  const [action, setAction] = useState<"start" | "stop" | "save" | null>(null);

  async function loadStatus(options: { silent?: boolean } = {}): Promise<{ error?: string }> {
    try {
      const result = await client.request(PROVIDER_IDS.code, "code_anvilStatus", [{}]);
      const parsed = parseAnvilStatus(result);
      setStatus(parsed);
      setConfig(parsed.config);
      return {};
    } catch (error) {
      if (!options.silent) {
        return { error: `Failed to load anvil status: ${asErrorMessage(error)}` };
      }
      return {};
    }
  }

  async function loadConfig(options: { silent?: boolean } = {}): Promise<{ error?: string }> {
    try {
      const result = await client.request(PROVIDER_IDS.code, "code_getAnvilConfig", [{}]);
      setConfig(parseAnvilConfig(result));
      return {};
    } catch (error) {
      if (!options.silent) {
        return { error: `Failed to load anvil config: ${asErrorMessage(error)}` };
      }
      return {};
    }
  }

  async function saveConfig(nextConfig: AnvilConfig): Promise<{ error?: string; status?: string }> {
    setAction("save");
    try {
      const result = await client.request(PROVIDER_IDS.code, "code_setAnvilConfig", [nextConfig]);
      const parsed = parseAnvilConfig(result);
      setConfig(parsed);
      console_.append([`[system] saved anvil config (port ${parsed.port}, chain ${parsed.chainId})`]);
      return { status: "Anvil config saved" };
    } catch (error) {
      const message = asErrorMessage(error);
      console_.append([`[system] failed to save anvil config: ${message}`]);
      return { error: `Failed to save anvil config: ${message}` };
    } finally {
      setAction(null);
    }
  }

  async function start(
    projectPath?: string,
    options: { auto?: boolean } = {}
  ): Promise<{ error?: string; status?: string }> {
    if (!projectPath?.trim()) {
      return { error: "Open a project before starting anvil." };
    }

    setAction("start");
    try {
      const result = await client.request(PROVIDER_IDS.code, "code_startAnvil", [{ projectPath }]);
      const parsed = parseAnvilStatus(result);
      setStatus(parsed);
      setConfig(parsed.config);
      console_.append([`[system] anvil starting on localhost:${parsed.port ?? parsed.config.port}`]);
      if (!options.auto) {
        return { status: "Anvil starting..." };
      }
      return {};
    } catch (error) {
      const message = asErrorMessage(error);
      console_.append([`[system] failed to start anvil: ${message}`]);
      return {
        error: options.auto
          ? `Project opened, but anvil failed to start: ${message}`
          : `Failed to start anvil: ${message}`,
      };
    } finally {
      setAction(null);
    }
  }

  async function stop(): Promise<{ error?: string; status?: string }> {
    setAction("stop");
    try {
      const result = await client.request(PROVIDER_IDS.code, "code_stopAnvil", [{}]);
      const parsed = parseAnvilStatus(result);
      setStatus(parsed);
      setConfig(parsed.config);
      console_.append(["[system] anvil stopped"]);
      return { status: "Anvil stopped" };
    } catch (error) {
      const message = asErrorMessage(error);
      console_.append([`[system] failed to stop anvil: ${message}`]);
      return { error: `Failed to stop anvil: ${message}` };
    } finally {
      setAction(null);
    }
  }

  return {
    status,
    config,
    action,
    setStatus,
    setConfig,
    loadStatus,
    loadConfig,
    saveConfig,
    start,
    stop,
  };
}
