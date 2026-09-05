import { afterEach, describe, expect, it, vi } from "vitest";
import { flushPromises, shallowMount, type VueWrapper } from "@vue/test-utils";
import { nextTick } from "vue";
import { NButton, NInput, NInputNumber, NSelect } from "naive-ui";
import SparkAnalyzerView from "../src/SparkAnalyzerView.vue";
import type { SparkAnalyzerAdapter, SparkAnalyzerPreferences, SparkAnalyzerPreferencesStore } from "../src/adapter";

vi.mock("naive-ui", async (importOriginal) => {
  const actual = await importOriginal<typeof import("naive-ui")>();
  return {
    ...actual,
    createDiscreteApi: () => ({ message: { success: vi.fn(), warning: vi.fn(), error: vi.fn() } }),
  };
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((accept, fail) => { resolve = accept; reject = fail; });
  return { promise, resolve, reject };
}

function makeAdapter() {
  return {
    loadReportBytes: vi.fn(), loadTextReport: vi.fn(), fetchReport: vi.fn(),
    executeTool: vi.fn(), runAnalysis: vi.fn(), cancelAnalysis: vi.fn(),
    askFollowUp: vi.fn(), testAiConnection: vi.fn(), listAiModels: vi.fn(),
    loadApiKey: vi.fn<SparkAnalyzerAdapter["loadApiKey"]>().mockResolvedValue(null),
    storeApiKey: vi.fn<SparkAnalyzerAdapter["storeApiKey"]>().mockResolvedValue(undefined),
    deleteApiKey: vi.fn<SparkAnalyzerAdapter["deleteApiKey"]>().mockResolvedValue(undefined),
    releaseReport: vi.fn(), pickSavePath: vi.fn(), saveExportFile: vi.fn(), openUrl: vi.fn(),
  } satisfies SparkAnalyzerAdapter;
}

const mounted: VueWrapper[] = [];
function mountView(store?: SparkAnalyzerPreferencesStore, adapter = makeAdapter()) {
  const wrapper = shallowMount(SparkAnalyzerView, {
    props: { adapter, preferencesStore: store, language: "en", embedded: true },
    global: { renderStubDefaultSlot: true },
  });
  mounted.push(wrapper);
  function input(fragment: string) {
    const field = wrapper.findAllComponents(NInput).find((item) => String(item.props("placeholder")).includes(fragment));
    if (!field) throw new Error(`Missing input ${fragment}`);
    return field;
  }
  async function edit(fragment: string, value: string) {
    input(fragment).vm.$emit("update:value", value);
    await nextTick();
  }
  async function save() {
    const button = wrapper.findAllComponents(NButton).find((item) => item.text() === "Save Config");
    if (!button) throw new Error("Missing save button");
    button.vm.$emit("click");
    await nextTick();
  }
  return { wrapper, adapter, input, edit, save };
}

afterEach(() => {
  for (const wrapper of mounted.splice(0)) wrapper.unmount();
  localStorage.clear();
});

describe("host preferences concurrency", () => {
  it("hydrates an untouched form and loads its endpoint credential once", async () => {
    const loading = deferred<SparkAnalyzerPreferences | null>();
    const adapter = makeAdapter();
    adapter.loadApiKey.mockResolvedValue("stored-key");
    const view = mountView({ load: () => loading.promise, save: vi.fn() }, adapter);
    loading.resolve({ providerId: "openai", base_url: "https://api.openai.com/v1", model: "stored-model", temperature: 0.4 });
    await flushPromises();
    expect(view.input("Base URL").props("value")).toBe("https://api.openai.com/v1");
    expect(view.input("API Key").props("value")).toBe("stored-key");
    expect(adapter.loadApiKey).toHaveBeenCalledExactlyOnceWith("https://api.openai.com/v1");
  });

  it("keeps a typed key when the initial preferences read finishes", async () => {
    const loading = deferred<SparkAnalyzerPreferences | null>();
    const view = mountView({ load: () => loading.promise, save: vi.fn() });
    await view.edit("API Key", "typed-key");
    loading.resolve({ base_url: "https://stored.example/v1" });
    await flushPromises();
    expect(view.input("API Key").props("value")).toBe("typed-key");
    expect(view.adapter.loadApiKey).not.toHaveBeenCalled();
  });

  for (const field of ["model", "temperature"] as const) {
    it(`initializes credentials for the unchanged endpoint after a ${field}-only edit`, async () => {
      const loading = deferred<SparkAnalyzerPreferences | null>();
      const adapter = makeAdapter();
      adapter.loadApiKey.mockResolvedValue("current-endpoint-key");
      const view = mountView({ load: () => loading.promise, save: vi.fn() }, adapter);
      const control = field === "model"
        ? view.wrapper.findAllComponents(NSelect).find((item) => Boolean(item.props("tag")))!
        : view.wrapper.findComponent(NInputNumber);
      const edited = field === "model" ? "edited-model" : 0.8;
      control.vm.$emit("update:value", edited);
      await nextTick();
      const unchangedEndpoint = view.input("Base URL").props("value");
      loading.resolve({ base_url: "https://stale.example/v1", model: "stale-model", temperature: 0.1 });
      await flushPromises();
      expect(control.props("value")).toBe(edited);
      expect(view.input("Base URL").props("value")).toBe(unchangedEndpoint);
      expect(adapter.loadApiKey).toHaveBeenCalledExactlyOnceWith(unchangedEndpoint);
      expect(view.input("API Key").props("value")).toBe("current-endpoint-key");
    });
  }

  it("saves the captured endpoint and key even when the provider changes during save", async () => {
    const saving = deferred<void>();
    const store = { load: vi.fn().mockResolvedValue(null), save: vi.fn().mockReturnValue(saving.promise) };
    const view = mountView(store);
    await flushPromises();
    await view.edit("Base URL", "https://first.example/v1");
    await flushPromises();
    await view.edit("API Key", "first-key");
    await view.save();
    view.wrapper.findAllComponents(NSelect).find((item) => item.props("value") === "custom")!
      .vm.$emit("update:value", "openai");
    await flushPromises();
    expect(view.input("API Key").props("value")).toBe("");
    saving.resolve(undefined);
    await flushPromises();
    expect(store.save).toHaveBeenCalledWith(expect.objectContaining({ base_url: "https://first.example/v1" }));
    expect(view.adapter.storeApiKey).toHaveBeenCalledWith("first-key", "https://first.example/v1");
    expect(view.adapter.deleteApiKey).not.toHaveBeenCalled();
  });

  for (const outcome of ["resolve", "reject"] as const) {
    it(`blocks overlapping saves and permits another after the first will ${outcome}`, async () => {
      const saving = deferred<void>();
      const store = { load: vi.fn().mockResolvedValue(null), save: vi.fn().mockReturnValueOnce(saving.promise).mockResolvedValue(undefined) };
      const view = mountView(store);
      await flushPromises();
      await view.edit("API Key", "first-key");
      await view.save();
      await view.edit("API Key", "");
      await view.save();
      expect(store.save).toHaveBeenCalledTimes(1);
      expect(view.adapter.deleteApiKey).not.toHaveBeenCalled();
      if (outcome === "resolve") saving.resolve(undefined);
      else saving.reject(new Error("host unavailable"));
      await flushPromises();
      expect(view.adapter.storeApiKey).toHaveBeenCalledTimes(outcome === "resolve" ? 1 : 0);
      await view.save();
      await flushPromises();
      expect(store.save).toHaveBeenCalledTimes(2);
      expect(view.adapter.deleteApiKey).toHaveBeenCalledTimes(1);
    });

    it(`preserves edits and clears old keys while a delayed load will ${outcome}`, async () => {
      const loading = deferred<SparkAnalyzerPreferences | null>();
      const view = mountView({ load: () => loading.promise, save: vi.fn() });
      await view.edit("API Key", "old-key");
      await view.edit("Base URL", "https://edited.example/v1");
      expect(view.input("API Key").props("value")).toBe("");
      await flushPromises();
      await view.edit("API Key", "edited-key");
      if (outcome === "resolve") loading.resolve({ base_url: "https://stale.example/v1", model: "stale-model" });
      else loading.reject(new Error("host unavailable"));
      await flushPromises();
      expect(view.input("Base URL").props("value")).toBe("https://edited.example/v1");
      expect(view.input("API Key").props("value")).toBe("edited-key");
      expect(view.adapter.loadApiKey).toHaveBeenCalledTimes(1);
      expect(view.adapter.loadApiKey).toHaveBeenCalledWith("https://edited.example/v1");
    });
  }

  it("does not load credentials after unmounting during the preferences read", async () => {
    const loading = deferred<SparkAnalyzerPreferences | null>();
    const view = mountView({ load: () => loading.promise, save: vi.fn() });
    view.wrapper.unmount();
    loading.resolve({ base_url: "https://stale.example/v1" });
    await flushPromises();
    expect(view.adapter.loadApiKey).not.toHaveBeenCalled();
  });
});
