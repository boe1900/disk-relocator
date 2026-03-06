import { flushPromises, mount } from "@vue/test-utils";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { defineComponent } from "vue";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "../../src/App.vue";
import { useI18n } from "../../src/i18n";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  convertFileSrc: vi.fn((input: string) => `asset://${input}`)
}));

const AppListStub = defineComponent({
  props: {
    apps: { type: Array, required: true },
    loading: { type: Boolean, required: true },
    error: { type: String, default: null }
  },
  emits: ["refresh", "migrate", "restore"],
  template: `
    <div>
      <div data-test="app-count">{{ apps.length }}</div>
      <div data-test="first-app-name">{{ apps[0]?.name || '' }}</div>
      <div data-test="first-app-target">{{ apps[0]?.targetDisk || '' }}</div>
      <div data-test="first-app-path">{{ apps[0]?.path || '' }}</div>
      <div data-test="first-app-desc">{{ apps[0]?.desc || '' }}</div>
      <div data-test="first-app-icon">{{ apps[0]?.icon || '' }}</div>
      <button data-test="emit-refresh" @click="$emit('refresh')">refresh</button>
      <button data-test="emit-migrate" @click="$emit('migrate', 'wechat-non-mas')">migrate</button>
      <button data-test="emit-restore" @click="$emit('restore', 'wechat-non-mas')">restore</button>
      <div data-test="incoming-error">{{ error || '' }}</div>
    </div>
  `
});

const MigrationDialogStub = defineComponent({
  props: {
    showModal: { type: Boolean, required: true }
  },
  emits: ["close", "done"],
  template: `
    <div>
      <div data-test="dialog-state">{{ showModal ? 'open' : 'closed' }}</div>
      <button data-test="emit-close" @click="$emit('close')">close</button>
      <button data-test="emit-done" @click="$emit('done', 'done-message')">done</button>
    </div>
  `
});

const HealthPanelStub = defineComponent({ template: `<div data-test="health-panel">health</div>` });
const LogPanelStub = defineComponent({ template: `<div data-test="log-panel">logs</div>` });

function makeInvokeMock(options: {
  listRelocations?: Array<Record<string, unknown>>;
  scanApps?: Array<Record<string, unknown>>;
  systemDisk?: Record<string, unknown>;
  systemDiskShouldFail?: boolean;
  rollbackShouldFail?: boolean;
}) {
  const listRelocations = options.listRelocations ?? [];
  const scanApps = options.scanApps ?? [
    {
      app_id: "wechat-non-mas",
      display_name: "WeChat",
      icon_path: "/Applications/WeChat.app/icon.icns",
      icon_data_url: null,
      tier: "experimental",
      detected_paths: [
        {
          path: "/Users/test/Library/Containers/com.tencent.xinWeChat",
          exists: true,
          is_symlink: true,
          size_bytes: 512
        }
      ],
      running: false,
      allow_bootstrap_if_source_missing: false,
      last_verified_at: "2026-03-06T10:00:00Z"
    }
  ];
  const systemDisk = options.systemDisk ?? {
    mount_point: "/",
    display_name: "Macintosh HD",
    is_mounted: true,
    is_writable: true,
    free_bytes: 300,
    total_bytes: 1000
  };
  const systemDiskShouldFail = options.systemDiskShouldFail ?? false;
  const rollbackShouldFail = options.rollbackShouldFail ?? false;

  return async (command: string, payload?: Record<string, unknown>) => {
    if (command === "scan_apps") {
      return scanApps;
    }

    if (command === "get_disk_status") {
      return [
        {
          mount_point: "/Volumes/M4_Ext_SSD",
          display_name: "M4_Ext_SSD",
          is_mounted: true,
          is_writable: true,
          free_bytes: 100,
          total_bytes: 200
        }
      ];
    }

    if (command === "list_relocations") {
      return listRelocations;
    }

    if (command === "get_system_disk_status") {
      if (systemDiskShouldFail) {
        throw new Error("permission denied");
      }
      return systemDisk;
    }

    if (command === "rollback_relocation") {
      if (rollbackShouldFail) {
        throw new Error("rollback failed");
      }
      return {
        relocation_id: (payload?.req as { relocation_id: string }).relocation_id,
        app_id: "wechat-non-mas",
        state: "ROLLED_BACK",
        health_state: "healthy",
        source_path: "/Users/test/source",
        target_path: "/Volumes/M4_Ext_SSD/target",
        backup_path: null,
        trace_id: "tr_rb",
        started_at: "2026-03-06T10:00:00Z",
        updated_at: "2026-03-06T10:00:01Z"
      };
    }

    throw new Error(`unexpected command: ${command}`);
  };
}

describe("App", () => {
  const invokeMock = vi.mocked(invoke);
  const convertFileSrcMock = vi.mocked(convertFileSrc);

  beforeEach(() => {
    window.localStorage.clear();
    useI18n().setLocale("zh");
    invokeMock.mockReset();
    convertFileSrcMock.mockClear();
    vi.stubGlobal("confirm", vi.fn(() => true));
  });

  it("loads data, opens migration modal, and refreshes after migration done", async () => {
    invokeMock.mockImplementation(
      makeInvokeMock({
        listRelocations: [
          {
            relocation_id: "reloc_001",
            app_id: "wechat-non-mas",
            state: "HEALTHY",
            health_state: "healthy",
            source_path: "/Users/test/source",
            target_path: "/Volumes/M4_Ext_SSD/AppData/WeChat",
            updated_at: "2026-03-06T10:00:00Z"
          }
        ]
      })
    );

    const wrapper = mount(App, {
      global: {
        stubs: {
          AppListView: AppListStub,
          MigrationDialog: MigrationDialogStub,
          HealthPanelView: HealthPanelStub,
          OperationLogExportView: LogPanelStub
        }
      }
    });

    await flushPromises();

    expect(wrapper.get('[data-test="app-count"]').text()).toBe("1");
    expect(convertFileSrcMock).toHaveBeenCalledWith("/Applications/WeChat.app/icon.icns");

    await wrapper.get('[data-test="emit-migrate"]').trigger("click");
    expect(wrapper.get('[data-test="dialog-state"]').text()).toBe("open");

    await wrapper.get('[data-test="emit-done"]').trigger("click");
    await flushPromises();

    expect(wrapper.get('[data-test="dialog-state"]').text()).toBe("closed");
    expect(wrapper.text()).toContain("done-message");
    expect(invokeMock.mock.calls.filter(([name]) => name === "scan_apps").length).toBeGreaterThan(1);
  });

  it("shows rollback record missing when relocation record does not exist", async () => {
    invokeMock.mockImplementation(makeInvokeMock({ listRelocations: [] }));

    const wrapper = mount(App, {
      global: {
        stubs: {
          AppListView: AppListStub,
          MigrationDialog: MigrationDialogStub,
          HealthPanelView: HealthPanelStub,
          OperationLogExportView: LogPanelStub
        }
      }
    });

    await flushPromises();
    await wrapper.get('[data-test="emit-restore"]').trigger("click");
    await flushPromises();

    expect(wrapper.text()).toContain("未找到 WeChat 的回滚记录");
    expect(invokeMock.mock.calls.some(([name]) => name === "rollback_relocation")).toBe(false);
  });

  it("executes rollback and refreshes state after restore", async () => {
    invokeMock.mockImplementation(
      makeInvokeMock({
        listRelocations: [
          {
            relocation_id: "reloc_rollback_target",
            app_id: "wechat-non-mas",
            state: "HEALTHY",
            health_state: "healthy",
            source_path: "/Users/test/source",
            target_path: "/Volumes/M4_Ext_SSD/AppData/WeChat",
            updated_at: "2026-03-06T10:00:00Z"
          }
        ]
      })
    );

    const wrapper = mount(App, {
      global: {
        stubs: {
          AppListView: AppListStub,
          MigrationDialog: MigrationDialogStub,
          HealthPanelView: HealthPanelStub,
          OperationLogExportView: LogPanelStub
        }
      }
    });

    await flushPromises();
    await wrapper.get('[data-test="emit-restore"]').trigger("click");
    await flushPromises();

    expect(invokeMock).toHaveBeenCalledWith(
      "rollback_relocation",
      expect.objectContaining({
        req: {
          relocation_id: "reloc_rollback_target",
          force: true
        }
      })
    );
    expect(wrapper.text()).toContain("已回滚到系统盘");
    expect(invokeMock.mock.calls.filter(([name]) => name === "scan_apps").length).toBeGreaterThan(1);
  });

  it("does not call rollback when user cancels confirm dialog", async () => {
    vi.stubGlobal("confirm", vi.fn(() => false));
    invokeMock.mockImplementation(
      makeInvokeMock({
        listRelocations: [
          {
            relocation_id: "reloc_rollback_target",
            app_id: "wechat-non-mas",
            state: "HEALTHY",
            health_state: "healthy",
            source_path: "/Users/test/source",
            target_path: "/Volumes/M4_Ext_SSD/AppData/WeChat",
            updated_at: "2026-03-06T10:00:00Z"
          }
        ]
      })
    );

    const wrapper = mount(App, {
      global: {
        stubs: {
          AppListView: AppListStub,
          MigrationDialog: MigrationDialogStub,
          HealthPanelView: HealthPanelStub,
          OperationLogExportView: LogPanelStub
        }
      }
    });

    await flushPromises();
    await wrapper.get('[data-test="emit-restore"]').trigger("click");
    await flushPromises();

    expect(invokeMock.mock.calls.some(([name]) => name === "rollback_relocation")).toBe(false);
  });

  it("shows rollback failure error when rollback command throws", async () => {
    invokeMock.mockImplementation(
      makeInvokeMock({
        listRelocations: [
          {
            relocation_id: "reloc_rollback_target",
            app_id: "wechat-non-mas",
            state: "HEALTHY",
            health_state: "healthy",
            source_path: "/Users/test/source",
            target_path: "/Volumes/M4_Ext_SSD/AppData/WeChat",
            updated_at: "2026-03-06T10:00:00Z"
          }
        ],
        rollbackShouldFail: true
      })
    );

    const wrapper = mount(App, {
      global: {
        stubs: {
          AppListView: AppListStub,
          MigrationDialog: MigrationDialogStub,
          HealthPanelView: HealthPanelStub,
          OperationLogExportView: LogPanelStub
        }
      }
    });

    await flushPromises();
    await wrapper.get('[data-test="emit-restore"]').trigger("click");
    await flushPromises();

    expect(wrapper.text()).toContain("回滚失败");
  });

  it("switches tabs between app list, health and operation history", async () => {
    invokeMock.mockImplementation(makeInvokeMock({ listRelocations: [] }));

    const wrapper = mount(App, {
      global: {
        stubs: {
          AppListView: AppListStub,
          MigrationDialog: MigrationDialogStub,
          HealthPanelView: HealthPanelStub,
          OperationLogExportView: LogPanelStub
        }
      }
    });
    await flushPromises();

    expect(wrapper.find('[data-test="app-count"]').exists()).toBe(true);
    expect(wrapper.find('[data-test="health-panel"]').exists()).toBe(false);
    expect(wrapper.find('[data-test="log-panel"]').exists()).toBe(false);

    const healthTab = wrapper
      .findAll("button")
      .find((btn) => btn.text().includes("健康检查"));
    expect(healthTab).toBeDefined();
    await healthTab!.trigger("click");
    await flushPromises();
    expect(wrapper.find('[data-test="health-panel"]').exists()).toBe(true);

    const logsTab = wrapper
      .findAll("button")
      .find((btn) => btn.text().includes("操作记录"));
    expect(logsTab).toBeDefined();
    await logsTab!.trigger("click");
    await flushPromises();
    expect(wrapper.find('[data-test="log-panel"]').exists()).toBe(true);
  });

  it("switches locale to English and persists locale selection", async () => {
    invokeMock.mockImplementation(makeInvokeMock({ listRelocations: [] }));

    const wrapper = mount(App, {
      global: {
        stubs: {
          AppListView: AppListStub,
          MigrationDialog: MigrationDialogStub,
          HealthPanelView: HealthPanelStub,
          OperationLogExportView: LogPanelStub
        }
      }
    });
    await flushPromises();

    const enButton = wrapper
      .findAll("button")
      .find((btn) => btn.text().trim() === "English");
    expect(enButton).toBeDefined();
    await enButton!.trigger("click");
    await flushPromises();

    expect(wrapper.text()).toContain("Applications");
    expect(window.localStorage.getItem("disk-relocator.locale")).toBe("en");
  });

  it("shows load error when initial refresh fails", async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "scan_apps") {
        throw new Error("scan failed");
      }
      if (command === "get_system_disk_status") {
        return {
          mount_point: "/",
          display_name: "Macintosh HD",
          is_mounted: true,
          is_writable: true,
          free_bytes: 300,
          total_bytes: 1000
        };
      }
      if (command === "get_disk_status") {
        return [];
      }
      if (command === "list_relocations") {
        return [];
      }
      if (command === "rollback_relocation") {
        return {
          relocation_id: "none",
          app_id: "none",
          state: "ROLLED_BACK",
          health_state: "healthy",
          source_path: "",
          target_path: "",
          backup_path: null,
          trace_id: "tr_rb",
          started_at: "2026-03-06T10:00:00Z",
          updated_at: "2026-03-06T10:00:01Z"
        };
      }
      throw new Error(`unexpected command: ${command}`);
    });

    const wrapper = mount(App, {
      global: {
        stubs: {
          AppListView: AppListStub,
          MigrationDialog: MigrationDialogStub,
          HealthPanelView: HealthPanelStub,
          OperationLogExportView: LogPanelStub
        }
      }
    });
    await flushPromises();

    expect(wrapper.text()).toContain("数据加载失败");
    expect(wrapper.get('[data-test="incoming-error"]').text()).toContain("数据加载失败");
  });

  it("shows system disk unavailable hint when system disk status call fails", async () => {
    invokeMock.mockImplementation(
      makeInvokeMock({
        scanApps: [],
        listRelocations: [],
        systemDiskShouldFail: true
      })
    );

    const wrapper = mount(App, {
      global: {
        stubs: {
          AppListView: AppListStub,
          MigrationDialog: MigrationDialogStub,
          HealthPanelView: HealthPanelStub,
          OperationLogExportView: LogPanelStub
        }
      }
    });
    await flushPromises();

    expect(wrapper.text()).toContain("暂无法读取系统盘容量");
  });

  it("maps unknown profile with fallback fields and decodes relocation disk name", async () => {
    invokeMock.mockImplementation(
      makeInvokeMock({
        scanApps: [
          {
            app_id: "unknown-app",
            display_name: "Unknown App",
            icon_path: null,
            icon_data_url: null,
            tier: "supported",
            detected_paths: [],
            running: false,
            allow_bootstrap_if_source_missing: false,
            last_verified_at: "2026-03-06T10:00:00Z"
          }
        ],
        listRelocations: [
          {
            relocation_id: "reloc_unknown_001",
            app_id: "unknown-app",
            state: "HEALTHY",
            health_state: "healthy",
            source_path: "/Users/test/source",
            target_path: "/Volumes/M4%20Ext%20SSD/RelocatorData/Unknown",
            updated_at: "2026-03-06T10:00:00Z"
          }
        ]
      })
    );

    const wrapper = mount(App, {
      global: {
        stubs: {
          AppListView: AppListStub,
          MigrationDialog: MigrationDialogStub,
          HealthPanelView: HealthPanelStub,
          OperationLogExportView: LogPanelStub
        }
      }
    });
    await flushPromises();

    expect(wrapper.get('[data-test="first-app-name"]').text()).toBe("Unknown App");
    expect(wrapper.get('[data-test="first-app-target"]').text()).toBe("M4 Ext SSD");
    expect(wrapper.get('[data-test="first-app-path"]').text()).toContain("未检测到路径");
    expect(wrapper.get('[data-test="first-app-desc"]').text()).toBe("应用数据目录迁移项");
    expect(wrapper.get('[data-test="first-app-icon"]').text()).toBe("📦");
  });

  it("uses default migration-done message when dialog emits empty payload", async () => {
    invokeMock.mockImplementation(makeInvokeMock({ listRelocations: [] }));

    const wrapper = mount(App, {
      global: {
        stubs: {
          AppListView: AppListStub,
          MigrationDialog: MigrationDialogStub,
          HealthPanelView: HealthPanelStub,
          OperationLogExportView: LogPanelStub
        }
      }
    });
    await flushPromises();

    await wrapper.get('[data-test="emit-migrate"]').trigger("click");
    expect(wrapper.get('[data-test="dialog-state"]').text()).toBe("open");

    wrapper.findComponent(MigrationDialogStub).vm.$emit("done", "");
    await flushPromises();

    expect(wrapper.get('[data-test="dialog-state"]').text()).toBe("closed");
    expect(wrapper.text()).toContain("迁移完成：-");
  });

  it("renders system disk usage bar width from free/total bytes", async () => {
    invokeMock.mockImplementation(
      makeInvokeMock({
        scanApps: [],
        listRelocations: [],
        systemDisk: {
          mount_point: "/",
          display_name: "Macintosh HD",
          is_mounted: true,
          is_writable: true,
          free_bytes: 300,
          total_bytes: 1000
        }
      })
    );

    const wrapper = mount(App, {
      global: {
        stubs: {
          AppListView: AppListStub,
          MigrationDialog: MigrationDialogStub,
          HealthPanelView: HealthPanelStub,
          OperationLogExportView: LogPanelStub
        }
      }
    });
    await flushPromises();

    const usageBar = wrapper.find(".bg-blue-500.h-2.rounded-full.transition-all.duration-500");
    expect(usageBar.exists()).toBe(true);
    expect(usageBar.attributes("style")).toContain("70%");
  });

  it("shows system disk unavailable when total bytes are zero", async () => {
    invokeMock.mockImplementation(
      makeInvokeMock({
        scanApps: [],
        listRelocations: [],
        systemDisk: {
          mount_point: "/",
          display_name: "Macintosh HD",
          is_mounted: true,
          is_writable: true,
          free_bytes: 0,
          total_bytes: 0
        }
      })
    );

    const wrapper = mount(App, {
      global: {
        stubs: {
          AppListView: AppListStub,
          MigrationDialog: MigrationDialogStub,
          HealthPanelView: HealthPanelStub,
          OperationLogExportView: LogPanelStub
        }
      }
    });
    await flushPromises();

    expect(wrapper.text()).toContain("暂无法读取系统盘容量");
  });
});
