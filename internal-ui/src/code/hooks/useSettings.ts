import { useState } from "react";
import type React from "react";
import type { ChatProvider } from "../chat/llm/provider";
import type { IpcClient } from "../../ipc/client";
import { PROVIDER_IDS } from "../../ipc/contracts";
import { asErrorMessage, asOptionalString, isRecord } from "../utils";
import { defaultModelForProvider, normalizeChatProvider, normalizeModelForProvider } from "../utils";

export interface SettingsHook {
  claudeApiKey: string;
  openaiApiKey: string;
  provider: ChatProvider;
  model: string;
  loading: boolean;
  saving: boolean;
  setClaudeApiKey: (key: string) => void;
  setOpenaiApiKey: (key: string) => void;
  handleProviderSelect: (event: React.ChangeEvent<HTMLSelectElement>) => void;
  setModel: (model: string) => void;
  load: (options?: { silent?: boolean }) => Promise<void>;
  save: () => Promise<{ error?: string }>;
}

export function useSettings(client: IpcClient): SettingsHook {
  const [claudeApiKey, setClaudeApiKey] = useState("");
  const [openaiApiKey, setOpenaiApiKey] = useState("");
  const [provider, setProvider] = useState<ChatProvider>("claude");
  const [model, setModel] = useState(defaultModelForProvider("claude"));
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);

  function handleProviderSelect(event: React.ChangeEvent<HTMLSelectElement>) {
    const nextProvider = normalizeChatProvider(event.target.value);
    setProvider(nextProvider);
    setModel((currentModel) => normalizeModelForProvider(nextProvider, currentModel));
  }

  async function load(options: { silent?: boolean } = {}): Promise<void> {
    setLoading(true);
    try {
      const [apiKeysResult, llmConfigResult] = await Promise.all([
        client.request(PROVIDER_IDS.code, "code_getApiKeys", [{}]),
        client.request(PROVIDER_IDS.code, "code_getLlmConfig", [{}]),
      ]);

      if (isRecord(apiKeysResult)) {
        setClaudeApiKey(asOptionalString(apiKeysResult.claude) ?? "");
        setOpenaiApiKey(asOptionalString(apiKeysResult.openai) ?? "");
      }

      if (isRecord(llmConfigResult)) {
        const nextProvider = normalizeChatProvider(asOptionalString(llmConfigResult.provider));
        setProvider(nextProvider);
        setModel(normalizeModelForProvider(nextProvider, asOptionalString(llmConfigResult.model)));
      }
    } catch (error) {
      if (!options.silent) {
        throw error;
      }
    } finally {
      setLoading(false);
    }
  }

  async function save(): Promise<{ error?: string }> {
    setSaving(true);
    try {
      await client.request(PROVIDER_IDS.code, "code_setApiKeys", [
        { claude: claudeApiKey, openai: openaiApiKey },
      ]);
      await client.request(PROVIDER_IDS.code, "code_setLlmConfig", [
        { provider, model: normalizeModelForProvider(provider, model) },
      ]);
      return {};
    } catch (error) {
      return { error: `Failed to save code settings: ${asErrorMessage(error)}` };
    } finally {
      setSaving(false);
    }
  }

  return {
    claudeApiKey,
    openaiApiKey,
    provider,
    model,
    loading,
    saving,
    setClaudeApiKey,
    setOpenaiApiKey,
    handleProviderSelect,
    setModel,
    load,
    save,
  };
}
