import { flushPromises, mount } from "@vue/test-utils";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import OperationLogExportView from "../../src/components/OperationLogExportView.vue";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn()
}));

describe("OperationLogExportView", () => {
  const invokeMock = vi.mocked(invoke);

  beforeEach(() => {
    window.localStorage.clear();
    invokeMock.mockReset();
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "list_relocations") {
        return [
          {
            relocation_id: "reloc_wechat_001",
            app_id: "wechat-non-mas",
            state: "HEALTHY",
            health_state: "healthy",
            source_path: "/Users/test/source",
            target_path: "/Volumes/Test/target",
            updated_at: "2026-03-06T10:10:00Z"
          },
          {
            relocation_id: "reloc_telegram_001",
            app_id: "telegram-desktop",
            state: "BROKEN",
            health_state: "broken",
            source_path: "/Users/test/tg",
            target_path: "/Volumes/Test/tg",
            updated_at: "2026-03-06T10:11:00Z"
          }
        ];
      }

      if (command === "list_operation_logs") {
        return [
          {
            log_id: "log_migrate_001",
            relocation_id: "reloc_wechat_001",
            trace_id: "tr_a",
            stage: "migration",
            step: "metadata_commit",
            status: "succeeded",
            error_code: null,
            duration_ms: 20,
            message: "migrate committed",
            details: {},
            created_at: "2026-03-06T10:00:02Z"
          },
          {
            log_id: "log_rollback_001",
            relocation_id: "reloc_telegram_001",
            trace_id: "tr_b",
            stage: "rollback",
            step: "restore_source",
            status: "failed",
            error_code: "ROLLBACK_RESTORE_BACKUP_FAILED",
            duration_ms: 9,
            message: "failed to restore source path from backup path.",
            details: {},
            created_at: "2026-03-06T10:01:10Z"
          }
        ];
      }

      throw new Error(`unexpected command: ${command}`);
    });
  });

  it("shows user-friendly operation records and expandable failure logs", async () => {
    const wrapper = mount(OperationLogExportView);
    await flushPromises();

    expect(wrapper.text()).toContain("迁移与回滚记录");
    expect(wrapper.text()).toContain("成功");
    expect(wrapper.text()).toContain("失败");
    expect(wrapper.text()).not.toContain("trace_id");
    expect(wrapper.text()).not.toContain("relocation_id");

    const expandBtn = wrapper
      .findAll("button")
      .find((btn) => btn.text().includes("查看失败日志"));
    expect(expandBtn).toBeDefined();

    await expandBtn!.trigger("click");
    await flushPromises();

    expect(wrapper.text()).toContain("ROLLBACK_RESTORE_BACKUP_FAILED");
    expect(wrapper.text()).toContain("failed to restore source path from backup path.");
  });

  it("filters records by operation type and app", async () => {
    const wrapper = mount(OperationLogExportView);
    await flushPromises();

    const selects = wrapper.findAll("select");
    expect(selects).toHaveLength(2);

    await selects[1].setValue("migrate");
    await flushPromises();
    const migrateFailureBtn = wrapper
      .findAll("button")
      .find((btn) => btn.text().includes("查看失败日志"));
    expect(migrateFailureBtn).toBeUndefined();

    await selects[1].setValue("rollback");
    await flushPromises();
    const rollbackFailureBtn = wrapper
      .findAll("button")
      .find((btn) => btn.text().includes("查看失败日志"));
    expect(rollbackFailureBtn).toBeDefined();

    await selects[0].setValue("wechat-non-mas");
    await flushPromises();
    const afterAppFilterFailureBtn = wrapper
      .findAll("button")
      .find((btn) => btn.text().includes("查看失败日志"));
    expect(afterAppFilterFailureBtn).toBeUndefined();
  });

  it("shows friendly error when operation log loading fails", async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "list_relocations") {
        return [];
      }
      if (command === "list_operation_logs") {
        throw new Error("network timeout");
      }
      throw new Error(`unexpected command: ${command}`);
    });

    const wrapper = mount(OperationLogExportView);
    await flushPromises();

    expect(wrapper.text()).toContain("拉取操作记录失败");
    expect(wrapper.text()).toContain("network timeout");
  });

  it("ignores health-stage logs and shows empty state", async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "list_relocations") {
        return [
          {
            relocation_id: "reloc_health_only",
            app_id: "wechat-non-mas",
            state: "HEALTHY",
            health_state: "healthy",
            source_path: "/Users/test/source",
            target_path: "/Volumes/Test/target",
            updated_at: "2026-03-06T10:11:00Z"
          }
        ];
      }
      if (command === "list_operation_logs") {
        return [
          {
            log_id: "log_health_001",
            relocation_id: "reloc_health_only",
            trace_id: "tr_h",
            stage: "health",
            step: "evaluate_relocation",
            status: "succeeded",
            error_code: null,
            duration_ms: 8,
            message: "health ok",
            details: {},
            created_at: "2026-03-06T10:01:10Z"
          }
        ];
      }
      throw new Error(`unexpected command: ${command}`);
    });

    const wrapper = mount(OperationLogExportView);
    await flushPromises();

    expect(wrapper.text()).toContain("暂无操作记录");
    expect(wrapper.text()).not.toContain("health ok");
  });

  it("marks pending migration as running when no success or failure commit exists", async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "list_relocations") {
        return [
          {
            relocation_id: "reloc_running_001",
            app_id: "wechat-non-mas",
            state: "COPYING",
            health_state: "unknown",
            source_path: "/Users/test/source",
            target_path: "/Volumes/Test/target",
            updated_at: "2026-03-06T10:11:00Z"
          }
        ];
      }
      if (command === "list_operation_logs") {
        return [
          {
            log_id: "log_running_001",
            relocation_id: "reloc_running_001",
            trace_id: "tr_running",
            stage: "migration",
            step: "copy_to_temp",
            status: "started",
            error_code: null,
            duration_ms: null,
            message: "copying",
            details: {},
            created_at: "2026-03-06T10:01:10Z"
          }
        ];
      }
      throw new Error(`unexpected command: ${command}`);
    });

    const wrapper = mount(OperationLogExportView);
    await flushPromises();

    expect(wrapper.text()).toContain("进行中");
  });

  it("can collapse failure details after expansion", async () => {
    const wrapper = mount(OperationLogExportView);
    await flushPromises();

    const expandBtn = wrapper
      .findAll("button")
      .find((btn) => btn.text().includes("查看失败日志"));
    expect(expandBtn).toBeDefined();
    await expandBtn!.trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("ROLLBACK_RESTORE_BACKUP_FAILED");

    const collapseBtn = wrapper
      .findAll("button")
      .find((btn) => btn.text().includes("收起失败日志"));
    expect(collapseBtn).toBeDefined();
    await collapseBtn!.trigger("click");
    await flushPromises();

    expect(wrapper.text()).not.toContain("ROLLBACK_RESTORE_BACKUP_FAILED");
  });

  it("falls back to unknown app name when relocation list is unavailable", async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "list_relocations") {
        throw new Error("db unavailable");
      }
      if (command === "list_operation_logs") {
        return [
          {
            log_id: "log_unknown_app_001",
            relocation_id: "reloc_missing",
            trace_id: "tr_unknown",
            stage: "rollback",
            step: "state_restore",
            status: "succeeded",
            error_code: null,
            duration_ms: 12,
            message: "rollback done",
            details: {},
            created_at: "2026-03-06T10:01:10Z"
          }
        ];
      }
      throw new Error(`unexpected command: ${command}`);
    });

    const wrapper = mount(OperationLogExportView);
    await flushPromises();

    expect(wrapper.text()).toContain("未知应用");
    expect(wrapper.text()).toContain("回滚");
    expect(wrapper.text()).toContain("成功");
  });

  it("filters out non-migration and non-rollback traces as unknown actions", async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "list_relocations") {
        return [];
      }
      if (command === "list_operation_logs") {
        return [
          {
            log_id: "log_misc_001",
            relocation_id: "reloc_misc",
            trace_id: "tr_misc",
            stage: "audit",
            step: "audit_check",
            status: "succeeded",
            error_code: null,
            duration_ms: 5,
            message: "audit complete",
            details: {},
            created_at: "2026-03-06T10:01:10Z"
          }
        ];
      }
      throw new Error(`unexpected command: ${command}`);
    });

    const wrapper = mount(OperationLogExportView);
    await flushPromises();

    expect(wrapper.text()).toContain("暂无操作记录");
    expect(wrapper.text()).not.toContain("audit complete");
  });

  it("keeps raw time text when created_at is not a valid date", async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "list_relocations") {
        return [
          {
            relocation_id: "reloc_invalid_time",
            app_id: "wechat-non-mas",
            state: "HEALTHY",
            health_state: "healthy",
            source_path: "/Users/test/source",
            target_path: "/Volumes/Test/target",
            updated_at: "2026-03-06T10:10:00Z"
          }
        ];
      }
      if (command === "list_operation_logs") {
        return [
          {
            log_id: "log_invalid_time",
            relocation_id: "reloc_invalid_time",
            trace_id: "tr_invalid",
            stage: "migration",
            step: "metadata_commit",
            status: "succeeded",
            error_code: null,
            duration_ms: 20,
            message: "done",
            details: {},
            created_at: "NOT_A_TIME"
          }
        ];
      }
      throw new Error(`unexpected command: ${command}`);
    });

    const wrapper = mount(OperationLogExportView);
    await flushPromises();

    expect(wrapper.text()).toContain("NOT_A_TIME");
  });

  it("sorts timeline by ended time descending", async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "list_relocations") {
        return [
          {
            relocation_id: "reloc_older",
            app_id: "wechat-non-mas",
            state: "HEALTHY",
            health_state: "healthy",
            source_path: "/Users/test/source1",
            target_path: "/Volumes/Test/target1",
            updated_at: "2026-03-06T10:10:00Z"
          },
          {
            relocation_id: "reloc_newer",
            app_id: "telegram-desktop",
            state: "HEALTHY",
            health_state: "healthy",
            source_path: "/Users/test/source2",
            target_path: "/Volumes/Test/target2",
            updated_at: "2026-03-06T10:11:00Z"
          }
        ];
      }
      if (command === "list_operation_logs") {
        return [
          {
            log_id: "log_old_1",
            relocation_id: "reloc_older",
            trace_id: "tr_old",
            stage: "migration",
            step: "copy_to_temp",
            status: "succeeded",
            error_code: null,
            duration_ms: 11,
            message: "old step",
            details: {},
            created_at: "2026-03-06T10:00:00Z"
          },
          {
            log_id: "log_old_2",
            relocation_id: "reloc_older",
            trace_id: "tr_old",
            stage: "migration",
            step: "metadata_commit",
            status: "succeeded",
            error_code: null,
            duration_ms: 12,
            message: "old done",
            details: {},
            created_at: "2026-03-06T10:00:05Z"
          },
          {
            log_id: "log_new_1",
            relocation_id: "reloc_newer",
            trace_id: "tr_new",
            stage: "rollback",
            step: "restore_source",
            status: "started",
            error_code: null,
            duration_ms: 11,
            message: "new step",
            details: {},
            created_at: "2026-03-06T10:05:00Z"
          },
          {
            log_id: "log_new_2",
            relocation_id: "reloc_newer",
            trace_id: "tr_new",
            stage: "rollback",
            step: "state_restore",
            status: "succeeded",
            error_code: null,
            duration_ms: 12,
            message: "new done",
            details: {},
            created_at: "2026-03-06T10:05:05Z"
          }
        ];
      }
      throw new Error(`unexpected command: ${command}`);
    });

    const wrapper = mount(OperationLogExportView);
    await flushPromises();

    const html = wrapper.html();
    const newerIndex = html.indexOf("rollback/state_restore");
    const olderIndex = html.indexOf("migration/metadata_commit");
    expect(newerIndex).toBeGreaterThan(-1);
    expect(olderIndex).toBeGreaterThan(-1);
    expect(newerIndex).toBeLessThan(olderIndex);
  });

  it("limits expanded failure details to latest six logs", async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "list_relocations") {
        return [
          {
            relocation_id: "reloc_many_failed",
            app_id: "wechat-non-mas",
            state: "BROKEN",
            health_state: "broken",
            source_path: "/Users/test/source",
            target_path: "/Volumes/Test/target",
            updated_at: "2026-03-06T10:10:00Z"
          }
        ];
      }
      if (command === "list_operation_logs") {
        return Array.from({ length: 7 }).map((_, index) => ({
          log_id: `log_failed_${index}`,
          relocation_id: "reloc_many_failed",
          trace_id: "tr_many_failed",
          stage: "rollback",
          step: "restore_source",
          status: "failed",
          error_code: `ERR_${index}`,
          duration_ms: 10,
          message: `failed_${index}`,
          details: {},
          created_at: `2026-03-06T10:00:0${index}Z`
        }));
      }
      throw new Error(`unexpected command: ${command}`);
    });

    const wrapper = mount(OperationLogExportView);
    await flushPromises();

    const expandBtn = wrapper
      .findAll("button")
      .find((btn) => btn.text().includes("查看失败日志"));
    expect(expandBtn).toBeDefined();
    await expandBtn!.trigger("click");
    await flushPromises();

    expect(wrapper.text()).not.toContain("failed_0");
    expect(wrapper.text()).toContain("failed_1");
    expect(wrapper.text()).toContain("failed_6");
  });

  it("classifies precheck-only trace as migration action", async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "list_relocations") {
        return [
          {
            relocation_id: "reloc_precheck_only",
            app_id: "wechat-non-mas",
            state: "PRECHECKING",
            health_state: "unknown",
            source_path: "/Users/test/source",
            target_path: "/Volumes/Test/target",
            updated_at: "2026-03-06T10:10:00Z"
          }
        ];
      }
      if (command === "list_operation_logs") {
        return [
          {
            log_id: "log_precheck_only",
            relocation_id: "reloc_precheck_only",
            trace_id: "tr_precheck_only",
            stage: "precheck",
            step: "source_scan",
            status: "succeeded",
            error_code: null,
            duration_ms: 8,
            message: "precheck done",
            details: {},
            created_at: "2026-03-06T10:01:10Z"
          }
        ];
      }
      throw new Error(`unexpected command: ${command}`);
    });

    const wrapper = mount(OperationLogExportView);
    await flushPromises();

    expect(wrapper.text()).toContain("迁移");
    expect(wrapper.text()).toContain("进行中");
  });
});
