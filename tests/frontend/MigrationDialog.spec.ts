import { flushPromises, mount } from "@vue/test-utils";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import MigrationDialog from "../../src/components/MigrationDialog.vue";
import { useI18n } from "../../src/i18n";
import type { AppScanResult, DiskStatus } from "../../src/types/contracts";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn()
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn()
}));

function makeApp(overrides: Partial<AppScanResult> = {}): AppScanResult {
  return {
    app_id: "wechat-non-mas",
    display_name: "WeChat",
    icon_path: null,
    icon_data_url: null,
    availability: "active",
    detected_paths: [
      {
        path: "/Users/test/Library/Containers/com.tencent.xinWeChat",
        exists: true,
        is_symlink: false,
        size_bytes: 1024
      }
    ],
    running: false,
    allow_bootstrap_if_source_missing: false,
    last_verified_at: "2026-03-06T10:00:00Z",
    ...overrides
  };
}

const disks: DiskStatus[] = [
  {
    mount_point: "/Volumes/M4_Ext_SSD",
    display_name: "M4_Ext_SSD",
    is_mounted: true,
    is_writable: true,
    free_bytes: 1_000_000,
    total_bytes: 2_000_000
  },
  {
    mount_point: "/Volumes/Macintosh HD",
    display_name: "Macintosh HD",
    is_mounted: true,
    is_writable: false,
    free_bytes: 100_000,
    total_bytes: 2_000_000
  }
];

describe("MigrationDialog", () => {
  const invokeMock = vi.mocked(invoke);
  const openMock = vi.mocked(open);
  const clipboardWriteText = vi.fn();

  beforeEach(() => {
    window.localStorage.clear();
    useI18n().setLocale("zh");
    invokeMock.mockReset();
    openMock.mockReset();
    clipboardWriteText.mockReset();
    clipboardWriteText.mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText: clipboardWriteText },
      configurable: true
    });
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.runOnlyPendingTimers();
    vi.useRealTimers();
  });

  it("submits migrate request and emits done after success", async () => {
    invokeMock.mockResolvedValue({
      relocation_id: "reloc_wechat_001",
      app_id: "wechat-non-mas",
      state: "HEALTHY",
      health_state: "healthy",
      source_path: "/Users/test/source",
      target_path: "/Volumes/M4_Ext_SSD/DataDock/wechat",
      backup_path: null,
      trace_id: "tr_1",
      started_at: "2026-03-06T10:00:00Z",
      updated_at: "2026-03-06T10:00:02Z"
    });

    const wrapper = mount(MigrationDialog, {
      props: {
        showModal: true,
        selectedAppId: "wechat-non-mas",
        selectedApp: makeApp(),
        disks
      }
    });

    await flushPromises();

    expect(wrapper.text()).toContain("M4_Ext_SSD");
    expect(wrapper.text()).not.toContain("Macintosh HD");

    const startBtn = wrapper
      .findAll("button")
      .find((btn) => btn.text().includes("开始迁移"));
    expect(startBtn).toBeDefined();
    expect(startBtn!.attributes("disabled")).toBeUndefined();

    await startBtn!.trigger("click");
    await flushPromises();

    expect(invokeMock).toHaveBeenCalledWith(
      "migrate_app",
      expect.objectContaining({
        req: expect.objectContaining({
          app_id: "wechat-non-mas",
          target_root: "/Volumes/M4_Ext_SSD",
          mode: "migrate",
          confirm_high_risk: false,
          cleanup_backup_after_migrate: true
        })
      })
    );

    vi.advanceTimersByTime(250);
    await flushPromises();
    expect(wrapper.text()).toContain("迁移成功");

    const finishBtn = wrapper.findAll("button").find((btn) => btn.text().includes("完成"));
    expect(finishBtn).toBeDefined();
    await finishBtn!.trigger("click");

    expect(wrapper.emitted("done")).toHaveLength(1);
    expect(String(wrapper.emitted("done")?.[0]?.[0] ?? "")).toContain("WeChat");
  });

  it("auto-includes all executable relocation units and sends unit_id for each request", async () => {
    invokeMock.mockResolvedValue({
      relocation_id: "reloc_unit_batch_001",
      app_id: "wechat-non-mas",
      state: "HEALTHY",
      health_state: "healthy",
      source_path: "/Users/test/source",
      target_path: "/Volumes/M4_Ext_SSD/RelocatorData/wechat",
      backup_path: null,
      trace_id: "tr_units",
      started_at: "2026-03-06T10:00:00Z",
      updated_at: "2026-03-06T10:00:02Z"
    });

    const wrapper = mount(MigrationDialog, {
      props: {
        showModal: true,
        selectedAppId: "wechat-non-mas",
        selectedApp: makeApp({
          detected_paths: [
            {
              unit_id: "media-and-files",
              display_name: "Media and Files",
              default_enabled: true,
              path: "/Users/test/Library/Containers/com.tencent.xinWeChat/FileStorage",
              exists: true,
              is_symlink: false,
              size_bytes: 1024
            },
            {
              unit_id: "images",
              display_name: "Images",
              default_enabled: true,
              path: "/Users/test/Library/Containers/com.tencent.xinWeChat/Image",
              exists: true,
              is_symlink: false,
              size_bytes: 2048
            }
          ]
        }),
        disks
      }
    });

    await flushPromises();

    expect(wrapper.findAll('[data-test="unit-checkbox"]')).toHaveLength(0);
    expect(wrapper.text()).toContain("已选 2/2");

    const startBtn = wrapper
      .findAll("button")
      .find((btn) => btn.text().includes("开始迁移"));
    expect(startBtn).toBeDefined();
    await startBtn!.trigger("click");
    await flushPromises();

    expect(invokeMock).toHaveBeenCalledTimes(2);
    expect(invokeMock).toHaveBeenNthCalledWith(
      1,
      "migrate_app",
      expect.objectContaining({
        req: expect.objectContaining({
          app_id: "wechat-non-mas",
          unit_id: "media-and-files",
          mode: "migrate"
        })
      })
    );
    expect(invokeMock).toHaveBeenNthCalledWith(
      2,
      "migrate_app",
      expect.objectContaining({
        req: expect.objectContaining({
          app_id: "wechat-non-mas",
          unit_id: "images",
          mode: "migrate"
        })
      })
    );

    vi.advanceTimersByTime(250);
    await flushPromises();
    const finishBtn = wrapper.findAll("button").find((btn) => btn.text().includes("完成"));
    expect(finishBtn).toBeDefined();
    await finishBtn!.trigger("click");

    const donePayload = String(wrapper.emitted("done")?.[0]?.[0] ?? "");
    expect(donePayload).toContain("WeChat");
    expect(donePayload).not.toContain("wechat-non-mas / wechat-non-mas");
  });

  it("shows account tabs when multiple match_1 groups are detected", async () => {
    const wrapper = mount(MigrationDialog, {
      props: {
        showModal: true,
        selectedAppId: "wechat-non-mas",
        selectedApp: makeApp({
          detected_paths: [
            {
              unit_id: "wechat-msg-all::wxid_b",
              display_name: "msg-wxid-b",
              default_enabled: true,
              path: "/Users/test/wechat/wxid_b/msg",
              exists: true,
              is_symlink: false,
              size_bytes: 1024
            },
            {
              unit_id: "wechat-msg-all::wxid_a",
              display_name: "msg-wxid-a",
              default_enabled: true,
              path: "/Users/test/wechat/wxid_a/msg",
              exists: true,
              is_symlink: false,
              size_bytes: 2048
            }
          ]
        }),
        disks
      }
    });
    await flushPromises();

    expect(wrapper.text()).toContain("账号 wxid_a");
    expect(wrapper.text()).toContain("账号 wxid_b");
    expect(wrapper.text()).toContain("msg-wxid-a");
    expect(wrapper.text()).not.toContain("msg-wxid-b");

    const accountBTab = wrapper.findAll("button").find((btn) => btn.text().includes("账号 wxid_b"));
    expect(accountBTab).toBeDefined();
    await accountBTab!.trigger("click");
    await flushPromises();

    expect(wrapper.text()).toContain("msg-wxid-b");
    expect(wrapper.text()).not.toContain("msg-wxid-a");
  });

  it("does not show account tabs when no match_1 groups are present", async () => {
    const wrapper = mount(MigrationDialog, {
      props: {
        showModal: true,
        selectedAppId: "wechat-non-mas",
        selectedApp: makeApp({
          detected_paths: [
            {
              unit_id: "media-and-files",
              display_name: "Media and Files",
              default_enabled: true,
              path: "/Users/test/Library/Containers/com.tencent.xinWeChat/FileStorage",
              exists: true,
              is_symlink: false,
              size_bytes: 1024
            },
            {
              unit_id: "images",
              display_name: "Images",
              default_enabled: true,
              path: "/Users/test/Library/Containers/com.tencent.xinWeChat/Image",
              exists: true,
              is_symlink: false,
              size_bytes: 2048
            }
          ]
        }),
        disks
      }
    });
    await flushPromises();

    const accountTabs = wrapper.findAll("button").filter((btn) => btn.text().includes("账号 "));
    expect(accountTabs).toHaveLength(0);
  });

  it("uses bootstrap mode when source is missing and rejects out-of-disk picker path", async () => {
    invokeMock.mockResolvedValue({
      relocation_id: "reloc_wechat_002",
      app_id: "wechat-non-mas",
      state: "HEALTHY",
      health_state: "healthy",
      source_path: "/Users/test/source",
      target_path: "/Volumes/M4_Ext_SSD/DataDock/wechat",
      backup_path: null,
      trace_id: "tr_2",
      started_at: "2026-03-06T10:00:00Z",
      updated_at: "2026-03-06T10:00:02Z"
    });
    openMock.mockResolvedValue("/tmp/not-under-selected-disk");

    const wrapper = mount(MigrationDialog, {
      props: {
        showModal: true,
        selectedAppId: "wechat-non-mas",
        selectedApp: makeApp({
          detected_paths: [
            {
              unit_id: "media-and-files",
              display_name: "Media and Files",
              default_enabled: true,
              allow_bootstrap_if_source_missing: true,
              path: "/Users/test/Library/Containers/com.tencent.xinWeChat/FileStorage",
              exists: false,
              is_symlink: false,
              size_bytes: 0
            }
          ],
          allow_bootstrap_if_source_missing: true
        }),
        disks
      }
    });

    await flushPromises();

    const pickBtn = wrapper
      .findAll("button")
      .find((btn) => btn.text().includes("系统选择"));
    expect(pickBtn).toBeDefined();
    await pickBtn!.trigger("click");
    await flushPromises();

    expect(openMock).toHaveBeenCalled();
    expect(wrapper.text()).toContain("所选路径不在目标盘 /Volumes/M4_Ext_SSD 下");

    const startBtn = wrapper
      .findAll("button")
      .find((btn) => btn.text().includes("开始迁移"));
    expect(startBtn).toBeDefined();
    await startBtn!.trigger("click");
    await flushPromises();

    expect(invokeMock).toHaveBeenCalledWith(
      "migrate_app",
      expect.objectContaining({
        req: expect.objectContaining({
          mode: "bootstrap"
        })
      })
    );
  });

  it("requires explicit confirmation for high-risk unit", async () => {
    invokeMock.mockResolvedValue({
      relocation_id: "reloc_wechat_003",
      app_id: "wechat-non-mas",
      state: "HEALTHY",
      health_state: "healthy",
      source_path: "/Users/test/source",
      target_path: "/Volumes/M4_Ext_SSD/DataDock/wechat",
      backup_path: null,
      trace_id: "tr_3",
      started_at: "2026-03-06T10:00:00Z",
      updated_at: "2026-03-06T10:00:02Z"
    });

    const wrapper = mount(MigrationDialog, {
      props: {
        showModal: true,
        selectedAppId: "wechat-non-mas",
        selectedApp: makeApp({
          detected_paths: [
            {
              unit_id: "media-and-files",
              display_name: "Media and Files",
              default_enabled: true,
              requires_confirmation: true,
              path: "/Users/test/Library/Containers/com.tencent.xinWeChat",
              exists: true,
              is_symlink: false,
              size_bytes: 1024
            }
          ]
        }),
        disks
      }
    });

    await flushPromises();

    const startBtn = wrapper
      .findAll("button")
      .find((btn) => btn.text().includes("开始迁移"));
    expect(startBtn).toBeDefined();
    expect(startBtn!.attributes("disabled")).toBeDefined();

    const confirmationCheckbox = wrapper.find('[data-test="confirm-high-risk-checkbox"]');
    expect(confirmationCheckbox.exists()).toBe(true);
    await confirmationCheckbox.setValue(true);
    await flushPromises();

    expect(startBtn!.attributes("disabled")).toBeUndefined();
    await startBtn!.trigger("click");
    await flushPromises();

    expect(invokeMock).toHaveBeenCalledWith(
      "migrate_app",
      expect.objectContaining({
        req: expect.objectContaining({
          confirm_high_risk: true
        })
      })
    );
  });

  it("shows no available disk warning and keeps start disabled", async () => {
    const wrapper = mount(MigrationDialog, {
      props: {
        showModal: true,
        selectedAppId: "wechat-non-mas",
        selectedApp: makeApp(),
        disks: [
          {
            mount_point: "/Volumes/Macintosh HD",
            display_name: "Macintosh HD",
            is_mounted: true,
            is_writable: false,
            free_bytes: 100_000,
            total_bytes: 2_000_000
          }
        ]
      }
    });
    await flushPromises();

    expect(wrapper.text()).toContain("当前没有可用于迁移的目标磁盘");
    const startBtn = wrapper
      .findAll("button")
      .find((btn) => btn.text().includes("开始迁移"));
    expect(startBtn).toBeDefined();
    expect(startBtn!.attributes("disabled")).toBeDefined();
  });

  it("skips blocked app migration plan with clear reason", async () => {
    const wrapper = mount(MigrationDialog, {
      props: {
        showModal: true,
        selectedAppId: "mas-sandbox-containers",
        selectedApp: makeApp({
          app_id: "mas-sandbox-containers",
          display_name: "MAS Sandbox Containers",
          availability: "blocked"
        }),
        disks
      }
    });
    await flushPromises();

    expect(wrapper.text()).toContain("本次将跳过");
    expect(wrapper.text()).toContain("blocked，不允许迁移");
    const startBtn = wrapper
      .findAll("button")
      .find((btn) => btn.text().includes("开始迁移"));
    expect(startBtn).toBeDefined();
    expect(startBtn!.attributes("disabled")).toBeDefined();
  });

  it("uses picked target path under selected disk when starting migration", async () => {
    invokeMock.mockResolvedValue({
      relocation_id: "reloc_wechat_004",
      app_id: "wechat-non-mas",
      state: "HEALTHY",
      health_state: "healthy",
      source_path: "/Users/test/source",
      target_path: "/Volumes/M4_Ext_SSD/RelocatorData/wechat",
      backup_path: null,
      trace_id: "tr_4",
      started_at: "2026-03-06T10:00:00Z",
      updated_at: "2026-03-06T10:00:02Z"
    });
    openMock.mockResolvedValue("/Volumes/M4_Ext_SSD/RelocatorData");

    const wrapper = mount(MigrationDialog, {
      props: {
        showModal: true,
        selectedAppId: "wechat-non-mas",
        selectedApp: makeApp(),
        disks
      }
    });
    await flushPromises();

    const pickBtn = wrapper
      .findAll("button")
      .find((btn) => btn.text().includes("系统选择"));
    expect(pickBtn).toBeDefined();
    await pickBtn!.trigger("click");
    await flushPromises();

    const startBtn = wrapper
      .findAll("button")
      .find((btn) => btn.text().includes("开始迁移"));
    expect(startBtn).toBeDefined();
    await startBtn!.trigger("click");
    await flushPromises();

    expect(invokeMock).toHaveBeenCalledWith(
      "migrate_app",
      expect.objectContaining({
        req: expect.objectContaining({
          target_root: "/Volumes/M4_Ext_SSD/RelocatorData"
        })
      })
    );
  });

  it("shows migration error when migrate command fails", async () => {
    invokeMock.mockRejectedValue(new Error("disk offline"));

    const wrapper = mount(MigrationDialog, {
      props: {
        showModal: true,
        selectedAppId: "wechat-non-mas",
        selectedApp: makeApp(),
        disks
      }
    });
    await flushPromises();

    const startBtn = wrapper
      .findAll("button")
      .find((btn) => btn.text().includes("开始迁移"));
    expect(startBtn).toBeDefined();
    await startBtn!.trigger("click");
    await flushPromises();

    expect(wrapper.text()).toContain("迁移失败");
    expect(wrapper.text()).toContain("disk offline");
    expect(wrapper.emitted("done")).toBeUndefined();
  });

  it("shows friendly target-path-exists message with path and trace id", async () => {
    invokeMock.mockRejectedValue({
      code: "PRECHECK_TARGET_PATH_EXISTS",
      message: "target path already exists.",
      trace_id: "tr_target_exists_1",
      details: {
        target_path: "/Volumes/M4_Ext_SSD/DataDock/AppData/WeChat/xwechat_files"
      }
    });

    const wrapper = mount(MigrationDialog, {
      props: {
        showModal: true,
        selectedAppId: "wechat-non-mas",
        selectedApp: makeApp(),
        disks
      }
    });
    await flushPromises();

    const startBtn = wrapper
      .findAll("button")
      .find((btn) => btn.text().includes("开始迁移"));
    expect(startBtn).toBeDefined();
    await startBtn!.trigger("click");
    await flushPromises();

    expect(wrapper.text()).toContain("目标目录已存在");
    expect(wrapper.text()).toContain("/Volumes/M4_Ext_SSD/DataDock/AppData/WeChat/xwechat_files");
    expect(wrapper.text()).toContain("trace_id=tr_target_exists_1");
  });

  it("renders command error object instead of [object Object]", async () => {
    invokeMock.mockRejectedValue({
      code: "MIGRATE_COPY_FAILED",
      message: "failed during copy source -> temp.",
      trace_id: "tr_object_error_1",
      details: {
        error: "Operation not permitted (os error 1)"
      }
    });

    const wrapper = mount(MigrationDialog, {
      props: {
        showModal: true,
        selectedAppId: "wechat-non-mas",
        selectedApp: makeApp(),
        disks
      }
    });
    await flushPromises();

    const startBtn = wrapper
      .findAll("button")
      .find((btn) => btn.text().includes("开始迁移"));
    expect(startBtn).toBeDefined();
    await startBtn!.trigger("click");
    await flushPromises();

    expect(wrapper.text()).toContain("MIGRATE_COPY_FAILED");
    expect(wrapper.text()).toContain("Operation not permitted (os error 1)");
    expect(wrapper.text()).toContain("trace_id=tr_object_error_1");
    expect(wrapper.text()).not.toContain("[object Object]");
  });

  it("emits close when cancel is clicked", async () => {
    const wrapper = mount(MigrationDialog, {
      props: {
        showModal: true,
        selectedAppId: "wechat-non-mas",
        selectedApp: makeApp(),
        disks
      }
    });
    await flushPromises();

    const cancelBtn = wrapper
      .findAll("button")
      .find((btn) => btn.text().includes("取消"));
    expect(cancelBtn).toBeDefined();
    await cancelBtn!.trigger("click");
    expect(wrapper.emitted("close")).toHaveLength(1);
  });

  it("does not render modal when showModal is false", () => {
    const wrapper = mount(MigrationDialog, {
      props: {
        showModal: false,
        selectedAppId: "wechat-non-mas",
        selectedApp: makeApp(),
        disks
      }
    });

    expect(wrapper.text()).toBe("");
    expect(wrapper.find(".fixed.inset-0").exists()).toBe(false);
  });

  it("does not render modal when selected app is null", () => {
    const wrapper = mount(MigrationDialog, {
      props: {
        showModal: true,
        selectedAppId: "wechat-non-mas",
        selectedApp: null,
        disks
      }
    });

    expect(wrapper.text()).toBe("");
    expect(wrapper.find(".fixed.inset-0").exists()).toBe(false);
  });

  it("shows select-disk-first error when picker clicked without available disk", async () => {
    const wrapper = mount(MigrationDialog, {
      props: {
        showModal: true,
        selectedAppId: "wechat-non-mas",
        selectedApp: makeApp(),
        disks: []
      }
    });
    await flushPromises();

    const pickBtn = wrapper
      .findAll("button")
      .find((btn) => btn.text().includes("系统选择"));
    expect(pickBtn).toBeDefined();
    await pickBtn!.trigger("click");
    await flushPromises();

    expect(wrapper.text()).toContain("请先选择目标盘，再使用系统选择");
    expect(openMock).not.toHaveBeenCalled();
  });

  it("skips running app migration with clear reason", async () => {
    const wrapper = mount(MigrationDialog, {
      props: {
        showModal: true,
        selectedAppId: "wechat-non-mas",
        selectedApp: makeApp({
          running: true
        }),
        disks
      }
    });
    await flushPromises();

    expect(wrapper.text()).toContain("应用运行中，请先退出");
    const startBtn = wrapper
      .findAll("button")
      .find((btn) => btn.text().includes("开始迁移"));
    expect(startBtn).toBeDefined();
    expect(startBtn!.attributes("disabled")).toBeDefined();
  });

  it("skips migration when source is missing and bootstrap is not allowed", async () => {
    const wrapper = mount(MigrationDialog, {
      props: {
        showModal: true,
        selectedAppId: "wechat-non-mas",
        selectedApp: makeApp({
          detected_paths: [],
          allow_bootstrap_if_source_missing: false
        }),
        disks
      }
    });
    await flushPromises();

    expect(wrapper.text()).toContain("未检测到源目录，当前应用还没有可迁移的数据");
    const startBtn = wrapper
      .findAll("button")
      .find((btn) => btn.text().includes("开始迁移"));
    expect(startBtn).toBeDefined();
    expect(startBtn!.attributes("disabled")).toBeDefined();
  });

  it("hides already migrated symlink paths from selectable migration units", async () => {
    const wrapper = mount(MigrationDialog, {
      props: {
        showModal: true,
        selectedAppId: "wechat-non-mas",
        selectedApp: makeApp({
          detected_paths: [
            {
              path: "/Users/test/Library/Containers/com.tencent.xinWeChat",
              exists: true,
              is_symlink: true,
              size_bytes: 0
            }
          ]
        }),
        disks
      }
    });
    await flushPromises();

    expect(wrapper.text()).not.toContain("源目录已是软链接，已迁移");
    expect(wrapper.findAll('[data-test="unit-checkbox"]')).toHaveLength(0);
    const startBtn = wrapper
      .findAll("button")
      .find((btn) => btn.text().includes("开始迁移"));
    expect(startBtn).toBeDefined();
    expect(startBtn!.attributes("disabled")).toBeDefined();
  });

  it("keeps form state unchanged when picker is canceled", async () => {
    openMock.mockResolvedValue(null);

    const wrapper = mount(MigrationDialog, {
      props: {
        showModal: true,
        selectedAppId: "wechat-non-mas",
        selectedApp: makeApp(),
        disks
      }
    });
    await flushPromises();

    const targetRootSelect = wrapper.find("select");
    expect((targetRootSelect.element as HTMLSelectElement).value).toBe("/Volumes/M4_Ext_SSD");

    const pickBtn = wrapper
      .findAll("button")
      .find((btn) => btn.text().includes("系统选择"));
    expect(pickBtn).toBeDefined();
    await pickBtn!.trigger("click");
    await flushPromises();

    expect(openMock).toHaveBeenCalled();
    expect((targetRootSelect.element as HTMLSelectElement).value).toBe("/Volumes/M4_Ext_SSD");
    expect(wrapper.text()).not.toContain("所选路径不在目标盘");
  });

  it("shows picker error when system dialog fails", async () => {
    openMock.mockRejectedValue(new Error("dialog unavailable"));

    const wrapper = mount(MigrationDialog, {
      props: {
        showModal: true,
        selectedAppId: "wechat-non-mas",
        selectedApp: makeApp(),
        disks
      }
    });
    await flushPromises();

    const pickBtn = wrapper
      .findAll("button")
      .find((btn) => btn.text().includes("系统选择"));
    expect(pickBtn).toBeDefined();
    await pickBtn!.trigger("click");
    await flushPromises();

    expect(wrapper.text()).toContain("dialog unavailable");
  });

  it("copies full source path to clipboard and shows copied feedback", async () => {
    const wrapper = mount(MigrationDialog, {
      props: {
        showModal: true,
        selectedAppId: "wechat-non-mas",
        selectedApp: makeApp(),
        disks
      }
    });
    await flushPromises();

    const copyBtn = wrapper.find('[data-test="copy-path-btn"]');
    expect(copyBtn.exists()).toBe(true);
    expect(copyBtn.text()).toBe("复制路径");

    await copyBtn.trigger("click");
    await flushPromises();

    expect(clipboardWriteText).toHaveBeenCalledWith(
      "/Users/test/Library/Containers/com.tencent.xinWeChat"
    );
    expect(copyBtn.text()).toBe("已复制");

    vi.advanceTimersByTime(1300);
    await flushPromises();
    expect(copyBtn.text()).toBe("复制路径");
  });

  it("opens source path in Finder from unit row", async () => {
    invokeMock.mockResolvedValue(undefined);

    const wrapper = mount(MigrationDialog, {
      props: {
        showModal: true,
        selectedAppId: "wechat-non-mas",
        selectedApp: makeApp(),
        disks
      }
    });
    await flushPromises();

    const openBtn = wrapper.find('[data-test="open-path-btn"]');
    expect(openBtn.exists()).toBe(true);
    expect(openBtn.text()).toBe("在 Finder 打开");

    await openBtn.trigger("click");
    await flushPromises();

    expect(invokeMock).toHaveBeenCalledWith("open_in_finder", {
      path: "/Users/test/Library/Containers/com.tencent.xinWeChat"
    });
  });

  it("keeps migration plan unchanged when clicking copy/open path actions", async () => {
    invokeMock.mockResolvedValue(undefined);

    const wrapper = mount(MigrationDialog, {
      props: {
        showModal: true,
        selectedAppId: "wechat-non-mas",
        selectedApp: makeApp({
          detected_paths: [
            {
              unit_id: "media-and-files",
              display_name: "Media and Files",
              default_enabled: true,
              path: "/Users/test/Library/Containers/com.tencent.xinWeChat/FileStorage",
              exists: true,
              is_symlink: false,
              size_bytes: 1024
            },
            {
              unit_id: "images",
              display_name: "Images",
              default_enabled: true,
              path: "/Users/test/Library/Containers/com.tencent.xinWeChat/Image",
              exists: true,
              is_symlink: false,
              size_bytes: 2048
            }
          ]
        }),
        disks
      }
    });
    await flushPromises();

    expect(wrapper.findAll('[data-test="unit-checkbox"]')).toHaveLength(0);
    const beforeCopyButtons = wrapper.findAll('[data-test="copy-path-btn"]');
    const beforeOpenButtons = wrapper.findAll('[data-test="open-path-btn"]');
    expect(beforeCopyButtons).toHaveLength(2);
    expect(beforeOpenButtons).toHaveLength(2);

    const copyBtn = wrapper.findAll('[data-test="copy-path-btn"]')[0];
    await copyBtn.trigger("click");
    await flushPromises();

    const openBtn = wrapper.findAll('[data-test="open-path-btn"]')[0];
    await openBtn.trigger("click");
    await flushPromises();

    const latestCopyButtons = wrapper.findAll('[data-test="copy-path-btn"]');
    const latestOpenButtons = wrapper.findAll('[data-test="open-path-btn"]');
    expect(latestCopyButtons).toHaveLength(2);
    expect(latestOpenButtons).toHaveLength(2);
  });

  it("passes cleanup_backup_after_migrate=false when user disables cleanup", async () => {
    invokeMock.mockResolvedValue({
      relocation_id: "reloc_wechat_cleanup_false",
      app_id: "wechat-non-mas",
      state: "HEALTHY",
      health_state: "healthy",
      source_path: "/Users/test/source",
      target_path: "/Volumes/M4_Ext_SSD/DataDock/wechat",
      backup_path: null,
      trace_id: "tr_cleanup",
      started_at: "2026-03-06T10:00:00Z",
      updated_at: "2026-03-06T10:00:02Z"
    });

    const wrapper = mount(MigrationDialog, {
      props: {
        showModal: true,
        selectedAppId: "wechat-non-mas",
        selectedApp: makeApp(),
        disks
      }
    });
    await flushPromises();

    const cleanupCheckbox = wrapper.find('[data-test="cleanup-checkbox"]');
    expect(cleanupCheckbox.exists()).toBe(true);
    await cleanupCheckbox.setValue(false);
    await flushPromises();

    const startBtn = wrapper
      .findAll("button")
      .find((btn) => btn.text().includes("开始迁移"));
    expect(startBtn).toBeDefined();
    await startBtn!.trigger("click");
    await flushPromises();

    expect(invokeMock).toHaveBeenCalledWith(
      "migrate_app",
      expect.objectContaining({
        req: expect.objectContaining({
          cleanup_backup_after_migrate: false
        })
      })
    );
  });
});
