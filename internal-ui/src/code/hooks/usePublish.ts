import { useState } from "react";
import type { IpcClient } from "../../ipc/client";
import { PROVIDER_IDS } from "../../ipc/contracts";
import type { IpfsPinConfig, PublishProgress } from "../types";
import { asErrorMessage } from "../utils";
import type { ConsoleHook } from "./useConsole";

const DEFAULT_IPFS_PIN_CONFIG: IpfsPinConfig = {
  endpoint: "http://127.0.0.1:5001",
  apiKey: null,
};

export interface PublishHook {
  ipfsConfig: IpfsPinConfig;
  setIpfsConfig: (config: IpfsPinConfig) => void;
  publishing: boolean;
  progress: PublishProgress | null;
  lastError: string | null;
  lastRootCid: string | null;
  savingConfig: boolean;
  loadIpfsConfig: (options?: { silent?: boolean }) => Promise<{ error?: string }>;
  saveIpfsConfig: (config: IpfsPinConfig) => Promise<{ error?: string; status?: string }>;
  proposeUpgrade: (projectPath: string) => Promise<{ error?: string; status?: string }>;
  setProgress: (p: PublishProgress | null) => void;
  setLastError: (e: string | null) => void;
  setLastRootCid: (c: string | null) => void;
}

export function usePublish(client: IpcClient, console_: ConsoleHook): PublishHook {
  const [ipfsConfig, setIpfsConfig] = useState<IpfsPinConfig>(DEFAULT_IPFS_PIN_CONFIG);
  const [publishing, setPublishing] = useState(false);
  const [progress, setProgress] = useState<PublishProgress | null>(null);
  const [lastError, setLastError] = useState<string | null>(null);
  const [lastRootCid, setLastRootCid] = useState<string | null>(null);
  const [savingConfig, setSavingConfig] = useState(false);

  async function loadIpfsConfig(options: { silent?: boolean } = {}): Promise<{ error?: string }> {
    try {
      const result = await client.request(PROVIDER_IDS.code, "code_getIpfsPinConfig", [{}]);
      if (result && typeof result === "object") {
        const r = result as Record<string, unknown>;
        setIpfsConfig({
          endpoint: typeof r.endpoint === "string" ? r.endpoint : DEFAULT_IPFS_PIN_CONFIG.endpoint,
          apiKey: typeof r.apiKey === "string" ? r.apiKey : null,
        });
      }
      return {};
    } catch (error) {
      if (!options.silent) {
        return { error: `Failed to load IPFS config: ${asErrorMessage(error)}` };
      }
      return {};
    }
  }

  async function saveIpfsConfig(config: IpfsPinConfig): Promise<{ error?: string; status?: string }> {
    setSavingConfig(true);
    try {
      const result = await client.request(PROVIDER_IDS.code, "code_setIpfsPinConfig", [config]);
      if (result && typeof result === "object") {
        const r = result as Record<string, unknown>;
        setIpfsConfig({
          endpoint: typeof r.endpoint === "string" ? r.endpoint : config.endpoint,
          apiKey: typeof r.apiKey === "string" ? r.apiKey : null,
        });
      }
      console_.append([`[system] IPFS pin config saved (${config.endpoint})`]);
      return { status: "IPFS config saved" };
    } catch (error) {
      const message = asErrorMessage(error);
      console_.append([`[system] failed to save IPFS config: ${message}`]);
      return { error: `Failed to save IPFS config: ${message}` };
    } finally {
      setSavingConfig(false);
    }
  }

  async function proposeUpgrade(projectPath: string): Promise<{ error?: string; status?: string }> {
    if (!projectPath.trim()) {
      return { error: "Open a project before proposing an upgrade." };
    }
    setPublishing(true);
    setProgress(null);
    setLastError(null);
    setLastRootCid(null);
    console_.append(["[system] starting propose-upgrade pipeline..."]);
    try {
      const result = await client.request(PROVIDER_IDS.code, "code_proposeUpgrade", [{ projectPath }]);
      const r = result && typeof result === "object" ? (result as Record<string, unknown>) : {};
      const rootCid = typeof r.rootCid === "string" ? r.rootCid : null;
      if (rootCid) {
        setLastRootCid(rootCid);
        console_.append([`[system] publish complete: ${rootCid}`]);
      }
      return { status: rootCid ? `Published: ${rootCid}` : "Publish complete" };
    } catch (error) {
      const message = asErrorMessage(error);
      setLastError(message);
      console_.append([`[system] publish failed: ${message}`]);
      return { error: `Publish failed: ${message}` };
    } finally {
      setPublishing(false);
      setProgress(null);
    }
  }

  return {
    ipfsConfig,
    setIpfsConfig,
    publishing,
    progress,
    lastError,
    lastRootCid,
    savingConfig,
    loadIpfsConfig,
    saveIpfsConfig,
    proposeUpgrade,
    setProgress,
    setLastError,
    setLastRootCid,
  };
}
