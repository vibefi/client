import type { IpcRequestMessage, ProviderId } from "./contracts";

type PendingCallback = {
  resolve: (value: unknown) => void;
  reject: (error: unknown) => void;
};

declare global {
  interface Window {
    ipc: {
      postMessage: (message: string) => void;
    };
  }
}

function postIpc(message: IpcRequestMessage) {
  try {
    window.ipc.postMessage(JSON.stringify(message));
  } catch (error) {
    console.error("[vibefi:ipc] failed to post message", error);
    throw error;
  }
}

export class IpcClient {
  private callbacks = new Map<number, PendingCallback>();
  private nextId = 1;

  request(providerId: ProviderId, method: string, params: unknown[] = []): Promise<unknown> {
    return new Promise((resolve, reject) => {
      const id = this.nextId++;
      this.callbacks.set(id, { resolve, reject });
      postIpc({
        id,
        providerId,
        method,
        params: Array.isArray(params) ? params : [],
      });
    });
  }

  notify(providerId: ProviderId, method: string, params: unknown[] = []) {
    postIpc({
      id: 0,
      providerId,
      method,
      params: Array.isArray(params) ? params : [],
    });
  }

  resolve(id: number, result: unknown, error: unknown) {
    const callback = this.callbacks.get(id);
    if (!callback) {
      console.warn("[vibefi:ipc] unresolved callback id", id);
      return;
    }
    this.callbacks.delete(id);
    if (error) callback.reject(error);
    else callback.resolve(result);
  }
}
