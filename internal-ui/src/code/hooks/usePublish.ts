import { useState } from "react";
import type { IpcClient } from "../../ipc/client";
import { PROVIDER_IDS } from "../../ipc/contracts";
import type { UploadConfig, PublishProgress } from "../types";
import { asErrorMessage } from "../utils";
import type { ConsoleHook } from "./useConsole";

const DEFAULT_UPLOAD_CONFIG: UploadConfig = {
  provider: "protocolRelay",
  protocolRelay: {
    endpoint: "",
    apiKey: null,
  },
  fourEverland: {
    endpoint: "https://api.4everland.dev",
    accessToken: null,
  },
  pinata: {
    endpoint: "https://api.pinata.cloud",
    apiKey: null,
  },
  localNode: {
    endpoint: "http://127.0.0.1:5001",
  },
};

export interface PublishHook {
  uploadConfig: UploadConfig;
  setUploadConfig: (config: UploadConfig) => void;
  publishing: boolean;
  progress: PublishProgress | null;
  lastError: string | null;
  lastRootCid: string | null;
  savingConfig: boolean;
  loadUploadConfig: (options?: { silent?: boolean }) => Promise<{ error?: string }>;
  saveUploadConfig: (config: UploadConfig) => Promise<{ error?: string; status?: string }>;
  proposeUpgrade: (projectPath: string) => Promise<{ error?: string; status?: string }>;
  setProgress: (p: PublishProgress | null) => void;
  setLastError: (e: string | null) => void;
  setLastRootCid: (c: string | null) => void;
}

export function usePublish(client: IpcClient, console_: ConsoleHook): PublishHook {
  const [uploadConfig, setUploadConfig] = useState<UploadConfig>(DEFAULT_UPLOAD_CONFIG);
  const [publishing, setPublishing] = useState(false);
  const [progress, setProgress] = useState<PublishProgress | null>(null);
  const [lastError, setLastError] = useState<string | null>(null);
  const [lastRootCid, setLastRootCid] = useState<string | null>(null);
  const [savingConfig, setSavingConfig] = useState(false);

  async function loadUploadConfig(options: { silent?: boolean } = {}): Promise<{ error?: string }> {
    try {
      const result = await client.request(PROVIDER_IDS.code, "code_getUploadConfig", [{}]);
      if (result && typeof result === "object") {
        setUploadConfig(result as UploadConfig);
      }
      return {};
    } catch (error) {
      if (!options.silent) {
        return { error: `Failed to load upload config: ${asErrorMessage(error)}` };
      }
      return {};
    }
  }

  async function saveUploadConfig(config: UploadConfig): Promise<{ error?: string; status?: string }> {
    setSavingConfig(true);
    try {
      const result = await client.request(PROVIDER_IDS.code, "code_setUploadConfig", [config]);
      if (result && typeof result === "object") {
        setUploadConfig(result as UploadConfig);
      }
      console_.append([`[system] upload config saved (${config.provider})`]);
      return { status: "Upload config saved" };
    } catch (error) {
      const message = asErrorMessage(error);
      console_.append([`[system] failed to save upload config: ${message}`]);
      return { error: `Failed to save upload config: ${message}` };
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
    uploadConfig,
    setUploadConfig,
    publishing,
    progress,
    lastError,
    lastRootCid,
    savingConfig,
    loadUploadConfig,
    saveUploadConfig,
    proposeUpgrade,
    setProgress,
    setLastError,
    setLastRootCid,
  };
}
