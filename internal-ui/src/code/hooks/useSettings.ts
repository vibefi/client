import { useCallback, useRef, useState } from "react";
import type React from "react";
import { Ollama } from "ollama/browser";
import type { ChatProvider, ReasoningEffort } from "../chat/llm/provider";
import { defaultModelForProvider, normalizeChatProvider, normalizeModelForProvider } from "../utils";

const STORAGE_KEY = "vibefi-code-llm-settings";

interface StoredSettings {
  claudeApiKey?: string;
  openaiApiKey?: string;
  openrouterApiKey?: string;
  ollamaPort?: number;
  provider?: string;
  model?: string;
  reasoningEffort?: string;
}

export interface SettingsHook {
  claudeApiKey: string;
  openaiApiKey: string;
  openrouterApiKey: string;
  ollamaPort: number;
  provider: ChatProvider;
  model: string;
  reasoningEffort: ReasoningEffort;
  loading: boolean;
  saving: boolean;
  ollamaModels: string[];
  ollamaModelsLoading: boolean;
  ollamaModelsError: string | null;
  setClaudeApiKey: (key: string) => void;
  setOpenaiApiKey: (key: string) => void;
  setOpenrouterApiKey: (key: string) => void;
  setOllamaPort: (port: number) => void;
  handleProviderSelect: (event: React.ChangeEvent<HTMLSelectElement>) => void;
  setModel: (model: string) => void;
  setReasoningEffort: (value: ReasoningEffort) => void;
  fetchOllamaModels: (port?: number) => Promise<void>;
  load: (options?: { silent?: boolean }) => Promise<void>;
  save: () => Promise<{ error?: string }>;
}

export function useSettings(): SettingsHook {
  const [claudeApiKey, setClaudeApiKey] = useState("");
  const [openaiApiKey, setOpenaiApiKey] = useState("");
  const [openrouterApiKey, setOpenrouterApiKey] = useState("");
  const [ollamaPort, setOllamaPort] = useState(11434);
  const [provider, setProvider] = useState<ChatProvider>("claude");
  const [model, setModel] = useState(defaultModelForProvider("claude"));
  const [reasoningEffort, setReasoningEffort] = useState<ReasoningEffort>("low");
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [ollamaModels, setOllamaModels] = useState<string[]>([]);
  const [ollamaModelsLoading, setOllamaModelsLoading] = useState(false);
  const [ollamaModelsError, setOllamaModelsError] = useState<string | null>(null);
  const ollamaFetchId = useRef(0);

  const fetchOllamaModels = useCallback(async (portOverride?: number) => {
    const p = portOverride ?? ollamaPort;
    const id = ++ollamaFetchId.current;
    setOllamaModelsLoading(true);
    setOllamaModelsError(null);
    try {
      const client = new Ollama({ host: `http://localhost:${p}` });
      const result = await client.list();
      if (id !== ollamaFetchId.current) return; // stale
      const names = result.models.map((m) => m.name).filter(Boolean);
      setOllamaModels(names);
      if (names.length > 0) {
        setModel((cur) => (names.includes(cur) ? cur : names[0]));
      }
    } catch (err) {
      if (id !== ollamaFetchId.current) return;
      setOllamaModels([]);
      setOllamaModelsError(err instanceof Error ? err.message : String(err));
    } finally {
      if (id === ollamaFetchId.current) setOllamaModelsLoading(false);
    }
  }, [ollamaPort]);

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
        setOpenrouterApiKey(stored.openrouterApiKey ?? "");
        if (typeof stored.ollamaPort === "number" && stored.ollamaPort > 0) {
          setOllamaPort(Math.trunc(stored.ollamaPort));
        }
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
        openrouterApiKey,
        ollamaPort,
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
    openrouterApiKey,
    ollamaPort,
    provider,
    model,
    reasoningEffort,
    loading,
    saving,
    ollamaModels,
    ollamaModelsLoading,
    ollamaModelsError,
    setClaudeApiKey,
    setOpenaiApiKey,
    setOpenrouterApiKey,
    setOllamaPort,
    handleProviderSelect,
    setModel,
    setReasoningEffort,
    fetchOllamaModels,
    load,
    save,
  };
}
