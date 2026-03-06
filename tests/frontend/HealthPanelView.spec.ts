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
  const rollbackRequests: Array<Record<string, unknown>> = [];

  beforeEach(() => {
    window.localStorage.clear();
    invokeMock.mockReset();
    reconcileRequests.length = 0;
    rollbackRequests.length = 0;
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

      if (command === "rollback_relocation") {
        const req = (payload?.req as Record<string, unknown>) ?? {};
        rollbackRequests.push(req);
        return {
          relocation_id: "reloc_test_001",
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
    });
  });

  afterEach(() => {
    vi.clearAllTimers();
    vi.useRealTimers();
  });

  it("runs self-check with auto-heal and supports rollback action", async () => {
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

    const rollbackBtn = wrapper
      .findAll("button")
      .find((btn) => btn.text().includes("一键回滚"));
    expect(rollbackBtn).toBeDefined();

    await rollbackBtn!.trigger("click");
    await flushPromises();

    expect(rollbackRequests.length).toBe(1);
    expect(rollbackRequests[0].relocation_id).toBe("reloc_test_001");
    expect(rollbackRequests[0].force).toBe(true);
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

  it("shows rollback error when rollback command fails", async () => {
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
      if (command === "reconcile_relocations") {
        return {
          trace_id: "tr_reconcile_boot",
          observed_at: "2026-03-06T10:00:00Z",
          scanned: 1,
          drift_count: 1,
          safe_fixable_count: 1,
          fixed_count: 1,
          issues: []
        };
      }
      if (command === "rollback_relocation") {
        throw new Error("rollback failed");
      }
      throw new Error(`unexpected command: ${command}`);
    });

    const wrapper = mount(HealthPanelView);
    await flushPromises();

    const rollbackBtn = wrapper
      .findAll("button")
      .find((btn) => btn.text().includes("一键回滚"));
    expect(rollbackBtn).toBeDefined();
    await rollbackBtn!.trigger("click");
    await flushPromises();

    expect(wrapper.text()).toContain("回滚失败");
    expect(wrapper.text()).toContain("rollback failed");
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
