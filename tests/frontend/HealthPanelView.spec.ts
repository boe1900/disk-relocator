import { flushPromises, mount } from "@vue/test-utils";
import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import HealthPanelView from "../../src/components/HealthPanelView.vue";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn()
}));

describe("HealthPanelView", () => {
  const invokeMock = vi.mocked(invoke);
  const reconcileRequests: Array<Record<string, unknown>> = [];

  beforeEach(() => {
    window.localStorage.clear();
    invokeMock.mockReset();
    reconcileRequests.length = 0;
    vi.useFakeTimers();

    invokeMock.mockImplementation(async (command: string, payload?: Record<string, unknown>) => {
      if (command === "get_disk_status") {
        return [
          {
            mount_point: "/Volumes/M4_Ext_SSD",
            display_name: "M4_Ext_SSD",
            is_mounted: true,
            is_writable: true,
            free_bytes: 1000,
            total_bytes: 2000
          }
        ];
      }

      if (command === "check_health") {
        return [
          {
            relocation_id: "reloc_test_001",
            app_id: "wechat-non-mas",
            state: "broken",
            checks: [{ code: "HEALTH_TARGET_MISSING", ok: false, message: "missing" }],
            observed_at: "2026-03-06T10:00:00Z"
          }
        ];
      }

      if (command === "list_health_events") {
        return [];
      }
      if (command === "list_relocations") {
        return [];
      }

      if (command === "reconcile_relocations") {
        const req = (payload?.req as Record<string, unknown>) ?? {};
        reconcileRequests.push(req);
        return {
          trace_id: "tr_reconcile_test",
          observed_at: "2026-03-06T10:00:00Z",
          scanned: 1,
          drift_count: 1,
          safe_fixable_count: 1,
          fixed_count: 1,
          issues: [
            {
              relocation_id: "reloc_test_001",
              app_id: "wechat-non-mas",
              code: "RECON_TEMP_PATH_RESIDUE",
              severity: "warning",
              message: "temp residue",
              suggestion: "auto fix",
              safe_fix_action: "cleanup_temp_path",
              safe_fix_applied: true,
              details: {}
            }
          ]
        };
      }

      if (command === "scan_apps") {
        return [];
      }

      throw new Error(`unexpected command: ${command}`);
    });
  });

  afterEach(() => {
    vi.clearAllTimers();
    vi.useRealTimers();
  });

  it("runs self-check with auto-heal", async () => {
    const wrapper = mount(HealthPanelView);
    await flushPromises();

    expect(reconcileRequests.length).toBeGreaterThan(0);
    expect(reconcileRequests[0].apply_safe_fixes).toBe(true);

    const selfCheckBtn = wrapper
      .findAll("button")
      .find((btn) => btn.text().includes("健康自检（自动纠偏）"));
    expect(selfCheckBtn).toBeDefined();

    await selfCheckBtn!.trigger("click");
    await flushPromises();
    expect(reconcileRequests.length).toBeGreaterThan(1);
    expect(wrapper.text()).toContain("自检完成");
    expect(wrapper.text()).toContain("当前严重异常 0 项");
    expect(wrapper.text()).not.toContain("一键回滚");
  });

  it("groups health items by app and uses display name with path action buttons", async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "get_disk_status") {
        return [
          {
            mount_point: "/Volumes/M4_Ext_SSD",
            display_name: "M4_Ext_SSD",
            is_mounted: true,
            is_writable: true,
            free_bytes: 1000,
            total_bytes: 2000
          }
        ];
      }
      if (command === "check_health") {
        return [
          {
            relocation_id: "reloc_wx_a",
            app_id: "wechat-non-mas",
            state: "healthy",
            checks: [{ code: "HEALTH_OK", ok: true, message: "ok" }],
            observed_at: "2026-03-06T10:00:00Z"
          },
          {
            relocation_id: "reloc_wx_b",
            app_id: "wechat-non-mas",
            state: "degraded",
            checks: [{ code: "HEALTH_TARGET_READONLY", ok: false, message: "readonly" }],
            observed_at: "2026-03-06T10:00:01Z"
          },
          {
            relocation_id: "reloc_tg_a",
            app_id: "telegram-desktop",
            state: "healthy",
            checks: [{ code: "HEALTH_OK", ok: true, message: "ok" }],
            observed_at: "2026-03-06T10:00:02Z"
          }
        ];
      }
      if (command === "list_health_events") {
        return [];
      }
      if (command === "scan_apps") {
        return [
          {
            app_id: "wechat-non-mas",
            display_name: "微信",
            availability: "active",
            detected_paths: [
              {
                unit_id: "wechat-msg-all::wxid_a",
                display_name: "聊天媒体资源库 [wxid_a]",
                path: "/Users/test/wechat/a/source",
                exists: true,
                is_symlink: true,
                size_bytes: 10
              },
              {
                unit_id: "wechat-msg-all::wxid_b",
                display_name: "聊天媒体资源库 [wxid_b]",
                path: "/Users/test/wechat/b/source",
                exists: true,
                is_symlink: true,
                size_bytes: 11
              }
            ],
            running: false,
            allow_bootstrap_if_source_missing: false,
            last_verified_at: "2026-03-06T10:00:00Z"
          },
          {
            app_id: "telegram-desktop",
            display_name: "Telegram",
            availability: "active",
            detected_paths: [
              {
                unit_id: "telegram-media::account-1",
                display_name: "媒体缓存 [account-1]",
                path: "/Users/test/telegram/a/source",
                exists: true,
                is_symlink: true,
                size_bytes: 12
              }
            ],
            running: false,
            allow_bootstrap_if_source_missing: false,
            last_verified_at: "2026-03-06T10:00:00Z"
          }
        ];
      }
      if (command === "list_relocations") {
        return [
          {
            relocation_id: "reloc_wx_a",
            app_id: "wechat-non-mas",
            state: "HEALTHY",
            health_state: "healthy",
            source_path: "/Users/test/wechat/a/source",
            target_path: "/Volumes/M4_Ext_SSD/wechat/a/target",
            source_size_bytes: 10,
            target_size_bytes: 10,
            updated_at: "2026-03-06T10:00:00Z"
          },
          {
            relocation_id: "reloc_wx_b",
            app_id: "wechat-non-mas",
            state: "DEGRADED",
            health_state: "degraded",
            source_path: "/Users/test/wechat/b/source",
            target_path: "/Volumes/M4_Ext_SSD/wechat/b/target",
            source_size_bytes: 11,
            target_size_bytes: 11,
            updated_at: "2026-03-06T10:00:01Z"
          },
          {
            relocation_id: "reloc_tg_a",
            app_id: "telegram-desktop",
            state: "HEALTHY",
            health_state: "healthy",
            source_path: "/Users/test/telegram/a/source",
            target_path: "/Volumes/M4_Ext_SSD/telegram/a/target",
            source_size_bytes: 12,
            target_size_bytes: 12,
            updated_at: "2026-03-06T10:00:02Z"
          }
        ];
      }
      if (command === "reconcile_relocations") {
        return {
          trace_id: "tr_reconcile_boot",
          observed_at: "2026-03-06T10:00:00Z",
          scanned: 3,
          drift_count: 0,
          safe_fixable_count: 0,
          fixed_count: 0,
          issues: []
        };
      }
      if (command === "rollback_relocation") {
        return {
          relocation_id: "reloc_wx_b",
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
      if (command === "scan_apps") {
        return [];
      }

      throw new Error(`unexpected command: ${command}`);
    });

    const wrapper = mount(HealthPanelView, {
      props: {
        appDisplayNames: {
          "wechat-non-mas": "微信",
          "telegram-desktop": "Telegram"
        }
      }
    });
    await flushPromises();

    expect(wrapper.text()).toContain("微信");
    expect(wrapper.text()).toContain("Telegram");
    expect(wrapper.text()).toContain("2 项迁移记录");
    expect(wrapper.text()).toContain("聊天媒体资源库");
    expect(wrapper.text()).toContain("账号 wxid_a");
    expect(wrapper.text()).toContain("在 Finder 打开");
    expect(wrapper.text()).toContain("复制路径");
    expect(wrapper.text()).not.toContain("[wxid_a]");
    expect(wrapper.text()).not.toContain("[wxid_b]");
  });

  it("supports mounted disk carousel and shows self-check error", async () => {
    let reconcileCallCount = 0;
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "get_disk_status") {
        return [
          {
            mount_point: "/Volumes/M4_Ext_SSD",
            display_name: "M4_Ext_SSD",
            is_mounted: true,
            is_writable: true,
            free_bytes: 1000,
            total_bytes: 2000
          },
          {
            mount_point: "/Volumes/Backup_SSD",
            display_name: "Backup_SSD",
            is_mounted: true,
            is_writable: true,
            free_bytes: 500,
            total_bytes: 1000
          }
        ];
      }

      if (command === "check_health") {
        return [
          {
            relocation_id: "reloc_test_healthy",
            app_id: "wechat-non-mas",
            state: "healthy",
            checks: [{ code: "HEALTH_OK", ok: true, message: "ok" }],
            observed_at: "2026-03-06T10:00:00Z"
          }
        ];
      }

      if (command === "list_health_events") {
        return [];
      }
      if (command === "list_relocations") {
        return [];
      }

      if (command === "reconcile_relocations") {
        reconcileCallCount += 1;
        if (reconcileCallCount === 1) {
          return {
            trace_id: "tr_reconcile_boot",
            observed_at: "2026-03-06T10:00:00Z",
            scanned: 1,
            drift_count: 0,
            safe_fixable_count: 0,
            fixed_count: 0,
            issues: []
          };
        }
        throw new Error("self-check failed");
      }

      if (command === "rollback_relocation") {
        return {
          relocation_id: "reloc_test_healthy",
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
      if (command === "scan_apps") {
        return [];
      }

      throw new Error(`unexpected command: ${command}`);
    });

    const wrapper = mount(HealthPanelView);
    await flushPromises();

    expect(wrapper.text()).toContain("M4_Ext_SSD");

    const pagerButtons = wrapper.findAll("button.p-1.rounded.border.border-gray-200");
    expect(pagerButtons.length).toBeGreaterThanOrEqual(2);

    await pagerButtons[1].trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("Backup_SSD");

    await pagerButtons[0].trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("M4_Ext_SSD");

    const selfCheckBtn = wrapper
      .findAll("button")
      .find((btn) => btn.text().includes("健康自检（自动纠偏）"));
    expect(selfCheckBtn).toBeDefined();
    await selfCheckBtn!.trigger("click");
    await flushPromises();

    expect(wrapper.text()).toContain("健康自检失败");
  });

  it("shows no-disk hint when there is no mounted writable target disk", async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "get_disk_status") {
        return [
          {
            mount_point: "/Volumes/Macintosh HD",
            display_name: "Macintosh HD",
            is_mounted: true,
            is_writable: false,
            free_bytes: 1000,
            total_bytes: 2000
          }
        ];
      }
      if (command === "check_health") {
        return [];
      }
      if (command === "list_health_events") {
        return [];
      }
      if (command === "list_relocations") {
        return [];
      }
      if (command === "reconcile_relocations") {
        return {
          trace_id: "tr_reconcile_boot",
          observed_at: "2026-03-06T10:00:00Z",
          scanned: 0,
          drift_count: 0,
          safe_fixable_count: 0,
          fixed_count: 0,
          issues: []
        };
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
      if (command === "scan_apps") {
        return [];
      }

      throw new Error(`unexpected command: ${command}`);
    });

    const wrapper = mount(HealthPanelView);
    await flushPromises();

    expect(wrapper.text()).toContain("暂无可迁移目标盘");
    expect(wrapper.text()).toContain("请连接并挂载可写目标磁盘后重试");
  });

  it("refreshes health payload on 10-second interval", async () => {
    let checkHealthCalls = 0;
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "get_disk_status") {
        return [
          {
            mount_point: "/Volumes/M4_Ext_SSD",
            display_name: "M4_Ext_SSD",
            is_mounted: true,
            is_writable: true,
            free_bytes: 1000,
            total_bytes: 2000
          }
        ];
      }
      if (command === "check_health") {
        checkHealthCalls += 1;
        return [];
      }
      if (command === "list_health_events") {
        return [];
      }
      if (command === "list_relocations") {
        return [];
      }
      if (command === "reconcile_relocations") {
        return {
          trace_id: "tr_reconcile_boot",
          observed_at: "2026-03-06T10:00:00Z",
          scanned: 0,
          drift_count: 0,
          safe_fixable_count: 0,
          fixed_count: 0,
          issues: []
        };
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
      if (command === "scan_apps") {
        return [];
      }

      throw new Error(`unexpected command: ${command}`);
    });

    mount(HealthPanelView);
    await flushPromises();
    const baseline = checkHealthCalls;
    expect(baseline).toBeGreaterThan(0);

    vi.advanceTimersByTime(10_100);
    await flushPromises();
    expect(checkHealthCalls).toBeGreaterThan(baseline);
  });

  it("shows at most six recent health events", async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "get_disk_status") {
        return [
          {
            mount_point: "/Volumes/M4_Ext_SSD",
            display_name: "M4_Ext_SSD",
            is_mounted: true,
            is_writable: true,
            free_bytes: 1000,
            total_bytes: 2000
          }
        ];
      }
      if (command === "check_health") {
        return [];
      }
      if (command === "list_health_events") {
        return Array.from({ length: 8 }).map((_, index) => ({
          snapshot_id: `snap_${index}`,
          relocation_id: "reloc_test_001",
          app_id: "wechat-non-mas",
          state: "healthy",
          check_code: `HEALTH_CODE_${index}`,
          message: "ok",
          observed_at: `2026-03-06T10:00:0${index}Z`
        }));
      }
      if (command === "list_relocations") {
        return [];
      }
      if (command === "reconcile_relocations") {
        return {
          trace_id: "tr_reconcile_boot",
          observed_at: "2026-03-06T10:00:00Z",
          scanned: 0,
          drift_count: 0,
          safe_fixable_count: 0,
          fixed_count: 0,
          issues: []
        };
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
      if (command === "scan_apps") {
        return [];
      }

      throw new Error(`unexpected command: ${command}`);
    });

    const wrapper = mount(HealthPanelView);
    await flushPromises();

    const codeMatches = wrapper.html().match(/HEALTH_CODE_/g) ?? [];
    expect(codeMatches).toHaveLength(6);
    expect(wrapper.text()).toContain("最近健康事件");
    expect(wrapper.text()).toContain("HEALTH_CODE_0");
    expect(wrapper.text()).not.toContain("HEALTH_CODE_7");
  });

  it("renders disk status as unmounted/readonly based on check code", async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "get_disk_status") {
        return [
          {
            mount_point: "/Volumes/M4_Ext_SSD",
            display_name: "M4_Ext_SSD",
            is_mounted: true,
            is_writable: true,
            free_bytes: 1000,
            total_bytes: 2000
          }
        ];
      }
      if (command === "check_health") {
        return [
          {
            relocation_id: "reloc_offline",
            app_id: "wechat-non-mas",
            state: "degraded",
            checks: [{ code: "HEALTH_DISK_OFFLINE", ok: false, message: "offline" }],
            observed_at: "2026-03-06T10:00:00Z"
          },
          {
            relocation_id: "reloc_readonly",
            app_id: "telegram-desktop",
            state: "degraded",
            checks: [{ code: "HEALTH_TARGET_READONLY", ok: false, message: "readonly" }],
            observed_at: "2026-03-06T10:00:01Z"
          }
        ];
      }
      if (command === "list_health_events") {
        return [];
      }
      if (command === "list_relocations") {
        return [];
      }
      if (command === "reconcile_relocations") {
        return {
          trace_id: "tr_reconcile_boot",
          observed_at: "2026-03-06T10:00:00Z",
          scanned: 2,
          drift_count: 0,
          safe_fixable_count: 0,
          fixed_count: 0,
          issues: []
        };
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
      if (command === "scan_apps") {
        return [];
      }

      throw new Error(`unexpected command: ${command}`);
    });

    const wrapper = mount(HealthPanelView);
    await flushPromises();

    expect(wrapper.text()).toContain("未挂载");
    expect(wrapper.text()).toContain("只读");
  });

  it("keeps panel usable when initial reconcile scan fails", async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "get_disk_status") {
        return [
          {
            mount_point: "/Volumes/M4_Ext_SSD",
            display_name: "M4_Ext_SSD",
            is_mounted: true,
            is_writable: true,
            free_bytes: 1000,
            total_bytes: 2000
          }
        ];
      }
      if (command === "check_health") {
        return [];
      }
      if (command === "list_health_events") {
        return [];
      }
      if (command === "list_relocations") {
        return [];
      }
      if (command === "reconcile_relocations") {
        throw new Error("reconcile boot failed");
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
      if (command === "scan_apps") {
        return [];
      }

      throw new Error(`unexpected command: ${command}`);
    });

    const wrapper = mount(HealthPanelView);
    await flushPromises();

    expect(wrapper.text()).toContain("健康检查与诊断");
    expect(wrapper.text()).toContain("暂无健康检查数据");
    expect(wrapper.text()).not.toContain("reconcile boot failed");
  });

  it("falls back to first mounted disk when disk list shrinks after refresh", async () => {
    let diskStatusCalls = 0;
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "get_disk_status") {
        diskStatusCalls += 1;
        if (diskStatusCalls < 3) {
          return [
            {
              mount_point: "/Volumes/M4_Ext_SSD",
              display_name: "M4_Ext_SSD",
              is_mounted: true,
              is_writable: true,
              free_bytes: 1000,
              total_bytes: 2000
            },
            {
              mount_point: "/Volumes/Backup_SSD",
              display_name: "Backup_SSD",
              is_mounted: true,
              is_writable: true,
              free_bytes: 500,
              total_bytes: 1000
            }
          ];
        }
        return [
          {
            mount_point: "/Volumes/M4_Ext_SSD",
            display_name: "M4_Ext_SSD",
            is_mounted: true,
            is_writable: true,
            free_bytes: 1000,
            total_bytes: 2000
          }
        ];
      }
      if (command === "check_health") {
        return [];
      }
      if (command === "list_health_events") {
        return [];
      }
      if (command === "list_relocations") {
        return [];
      }
      if (command === "reconcile_relocations") {
        return {
          trace_id: "tr_reconcile_boot",
          observed_at: "2026-03-06T10:00:00Z",
          scanned: 0,
          drift_count: 0,
          safe_fixable_count: 0,
          fixed_count: 0,
          issues: []
        };
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
      if (command === "scan_apps") {
        return [];
      }

      throw new Error(`unexpected command: ${command}`);
    });

    const wrapper = mount(HealthPanelView);
    await flushPromises();
    expect(wrapper.text()).toContain("M4_Ext_SSD");

    const pagerButtons = wrapper.findAll("button.p-1.rounded.border.border-gray-200");
    expect(pagerButtons.length).toBeGreaterThanOrEqual(2);
    await pagerButtons[1].trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("Backup_SSD");

    vi.advanceTimersByTime(10_100);
    await flushPromises();

    expect(wrapper.text()).toContain("M4_Ext_SSD");
    expect(wrapper.text()).not.toContain("Backup_SSD");
  });
});
