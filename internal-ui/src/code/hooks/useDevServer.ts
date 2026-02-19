import { useState } from "react";
import type { IpcClient } from "../../ipc/client";
import { PROVIDER_IDS } from "../../ipc/contracts";
import type { DevServerStatus } from "../types";
import { asErrorMessage, isRecord, parseDevServerStatus, parsePort } from "../utils";
import type { ConsoleHook } from "./useConsole";

export interface DevServerHook {
  status: DevServerStatus;
  action: "start" | "stop" | null;
  setStatus: (status: DevServerStatus) => void;
  start: (
    pathOverride?: string,
    options?: { auto?: boolean }
  ) => Promise<{ error?: string; status?: string }>;
  stop: () => Promise<{ error?: string; status?: string }>;
  loadStatus: (options?: { silent?: boolean }) => Promise<{ error?: string }>;
}

export function useDevServer(client: IpcClient, console_: ConsoleHook): DevServerHook {
  const [status, setStatus] = useState<DevServerStatus>({ running: false, port: null });
  const [action, setAction] = useState<"start" | "stop" | null>(null);

  async function loadStatus(options: { silent?: boolean } = {}): Promise<{ error?: string }> {
    try {
      const result = await client.request(PROVIDER_IDS.code, "code_devServerStatus", [{}]);
      setStatus(parseDevServerStatus(result));
      return {};
    } catch (error) {
      if (!options.silent) {
        return { error: `Failed to load dev server status: ${asErrorMessage(error)}` };
      }
      return {};
    }
  }

  async function start(
    pathOverride?: string,
    options: { auto?: boolean } = {}
  ): Promise<{ error?: string; status?: string }> {
    if (!pathOverride) {
      return { error: "Open a project before starting the dev server." };
    }

    const projectPath = pathOverride.trim();
    if (!projectPath) {
      return { error: "Open a project before starting the dev server." };
    }

    setAction("start");
    console_.append([`[system] starting dev server for ${projectPath}`]);

    try {
      const result = await client.request(PROVIDER_IDS.code, "code_startDevServer", [
        { projectPath },
      ]);
      const port = isRecord(result) ? parsePort(result.port) : null;
      setStatus({ running: true, port });
      if (port !== null) {
        console_.append([`[system] dev server process started (requested localhost:${port})`]);
      }
      await loadStatus({ silent: true });
      if (!options.auto) {
        return { status: "Dev server starting..." };
      }
      return {};
    } catch (error) {
      const message = asErrorMessage(error);
      console_.append([`[system] failed to start dev server: ${message}`]);
      return {
        error: options.auto
          ? `Project opened, but dev server failed to start: ${message}`
          : `Failed to start dev server: ${message}`,
      };
    } finally {
      setAction(null);
    }
  }

  async function stop(): Promise<{ error?: string; status?: string }> {
    setAction("stop");

    try {
      await client.request(PROVIDER_IDS.code, "code_stopDevServer", [{}]);
      setStatus({ running: false, port: null });
      console_.append(["[system] dev server stopped"]);
      await loadStatus({ silent: true });
      return { status: "Dev server stopped" };
    } catch (error) {
      const message = asErrorMessage(error);
      console_.append([`[system] failed to stop dev server: ${message}`]);
      return { error: `Failed to stop dev server: ${message}` };
    } finally {
      setAction(null);
    }
  }

  return { status, action, setStatus, start, stop, loadStatus };
}
