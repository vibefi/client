import { useState } from "react";
import type React from "react";
import type { ChatProvider, ReasoningEffort } from "../chat/llm/provider";
import { defaultModelForProvider, normalizeChatProvider, normalizeModelForProvider } from "../utils";

const STORAGE_KEY = "vibefi-code-llm-settings";

interface StoredSettings {
  claudeApiKey?: string;
  openaiApiKey?: string;
  provider?: string;
  model?: string;
  reasoningEffort?: string;
}

export interface SettingsHook {
  claudeApiKey: string;
  openaiApiKey: string;
  provider: ChatProvider;
  model: string;
  reasoningEffort: ReasoningEffort;
  loading: boolean;
  saving: boolean;
  setClaudeApiKey: (key: string) => void;
  setOpenaiApiKey: (key: string) => void;
  handleProviderSelect: (event: React.ChangeEvent<HTMLSelectElement>) => void;
  setModel: (model: string) => void;
  setReasoningEffort: (value: ReasoningEffort) => void;
  load: (options?: { silent?: boolean }) => Promise<void>;
  save: () => Promise<{ error?: string }>;
}

export function useSettings(): SettingsHook {
  const [claudeApiKey, setClaudeApiKey] = useState("");
  const [openaiApiKey, setOpenaiApiKey] = useState("");
  const [provider, setProvider] = useState<ChatProvider>("claude");
  const [model, setModel] = useState(defaultModelForProvider("claude"));
  const [reasoningEffort, setReasoningEffort] = useState<ReasoningEffort>("low");
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
      const raw = window.localStorage.getItem(STORAGE_KEY);
      if (raw) {
        const stored: StoredSettings = JSON.parse(raw);
        setClaudeApiKey(stored.claudeApiKey ?? "");
        setOpenaiApiKey(stored.openaiApiKey ?? "");
        const nextProvider = normalizeChatProvider(stored.provider);
        setProvider(nextProvider);
        setModel(normalizeModelForProvider(nextProvider, stored.model));
        const effort = stored.reasoningEffort;
        if (effort === "low" || effort === "medium" || effort === "high") {
          setReasoningEffort(effort);
        }
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
      const stored: StoredSettings = {
        claudeApiKey,
        openaiApiKey,
        provider,
        model: normalizeModelForProvider(provider, model),
        reasoningEffort,
      };
      window.localStorage.setItem(STORAGE_KEY, JSON.stringify(stored));
      return {};
    } catch (error) {
      return { error: `Failed to save code settings: ${error instanceof Error ? error.message : String(error)}` };
    } finally {
      setSaving(false);
    }
  }

  return {
    claudeApiKey,
    openaiApiKey,
    provider,
    model,
    reasoningEffort,
    loading,
    saving,
    setClaudeApiKey,
    setOpenaiApiKey,
    handleProviderSelect,
    setModel,
    setReasoningEffort,
    load,
    save,
  };
}
