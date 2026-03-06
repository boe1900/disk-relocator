<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { computed, onMounted, ref } from "vue";
import type { OperationLogItem, OperationLogsRequest, RelocationSummary } from "../types/contracts";
import { useI18n } from "../i18n";

type UserActionType = "migrate" | "rollback" | "unknown";
type UserActionResult = "success" | "failed" | "running";

interface TimelineItem {
  key: string;
  appName: string;
  action: UserActionType;
  result: UserActionResult;
  startedAt: string;
  endedAt: string;
  lastStep: string;
  failedLogs: OperationLogItem[];
}

const operationType = ref<"all" | "migrate" | "rollback">("all");
const selectedApp = ref("all");
const expandedFailedKey = ref<string | null>(null);

const relocations = ref<RelocationSummary[]>([]);
const recordsRaw = ref<OperationLogItem[]>([]);

const loadingRecords = ref(false);
const error = ref<string | null>(null);
const { t } = useI18n();

const relocationMap = computed(() => new Map(relocations.value.map((item) => [item.relocation_id, item])));

const appOptions = computed(() => {
  const set = new Set<string>();
  for (const item of relocations.value) {
    set.add(item.app_id);
  }
  return [...set].sort((a, b) => a.localeCompare(b));
});

function inferAction(logs: OperationLogItem[]): UserActionType {
  if (logs.some((log) => log.stage === "rollback")) {
    return "rollback";
  }
  if (logs.some((log) => log.stage === "migration" || log.stage === "precheck")) {
    return "migrate";
  }
  return "unknown";
}

function inferResult(action: UserActionType, logs: OperationLogItem[]): UserActionResult {
  const migrateCommitted = logs.some(
    (log) =>
      log.stage === "migration" && log.step === "metadata_commit" && log.status === "succeeded"
  );
  const rollbackCommitted = logs.some(
    (log) => log.stage === "rollback" && log.step === "state_restore" && log.status === "succeeded"
  );

  if ((action === "migrate" && migrateCommitted) || (action === "rollback" && rollbackCommitted)) {
    return "success";
  }
  if (logs.some((log) => log.status === "failed")) {
    return "failed";
  }
  return "running";
}

function formatTime(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return value;
  }
  return date.toLocaleString();
}

function actionLabel(action: UserActionType): string {
  if (action === "migrate") {
    return t("logs.action.migrate");
  }
  if (action === "rollback") {
    return t("logs.action.rollback");
  }
  return t("logs.action.unknown");
}

function resultLabel(result: UserActionResult): string {
  if (result === "success") {
    return t("logs.result.success");
  }
  if (result === "failed") {
    return t("logs.result.failed");
  }
  return t("logs.result.running");
}

function resultClass(result: UserActionResult): string {
  if (result === "success") {
    return "bg-green-100 text-green-700 border border-green-200";
  }
  if (result === "failed") {
    return "bg-red-100 text-red-700 border border-red-200";
  }
  return "bg-yellow-100 text-yellow-700 border border-yellow-200";
}

function toggleFailedDetail(key: string): void {
  expandedFailedKey.value = expandedFailedKey.value === key ? null : key;
}

const timelineItems = computed<TimelineItem[]>(() => {
  const groups = new Map<string, OperationLogItem[]>();
  for (const log of recordsRaw.value) {
    if (log.stage === "health") {
      continue;
    }
    const key = `${log.trace_id}::${log.relocation_id}`;
    const bucket = groups.get(key);
    if (bucket) {
      bucket.push(log);
    } else {
      groups.set(key, [log]);
    }
  }

  const items: TimelineItem[] = [];
  for (const [key, logs] of groups) {
    logs.sort((a, b) => a.created_at.localeCompare(b.created_at));
    const action = inferAction(logs);
    if (action === "unknown") {
      continue;
    }
    if (operationType.value !== "all" && action !== operationType.value) {
      continue;
    }

    const first = logs[0];
    const last = logs[logs.length - 1];
    const relocation = relocationMap.value.get(first.relocation_id);
    const appName = relocation?.app_id ?? t("logs.unknownApp");

    if (selectedApp.value !== "all" && appName !== selectedApp.value) {
      continue;
    }

    items.push({
      key,
      appName,
      action,
      result: inferResult(action, logs),
      startedAt: first.created_at,
      endedAt: last.created_at,
      lastStep: `${last.stage}/${last.step}`,
      failedLogs: logs.filter((log) => log.status === "failed")
    });
  }

  items.sort((a, b) => b.endedAt.localeCompare(a.endedAt));
  return items;
});

async function loadRelocations(): Promise<void> {
  try {
    relocations.value = await invoke<RelocationSummary[]>("list_relocations");
  } catch {
    // no-op
  }
}

async function loadOperationRecords(): Promise<void> {
  loadingRecords.value = true;
  error.value = null;

  const req: OperationLogsRequest = { limit: 1000 };
  try {
    recordsRaw.value = await invoke<OperationLogItem[]>("list_operation_logs", { req });
  } catch (err) {
    error.value = t("logs.errorListFailed", { error: String(err) });
  } finally {
    loadingRecords.value = false;
  }
}

async function refreshAll(): Promise<void> {
  await Promise.all([loadRelocations(), loadOperationRecords()]);
}

onMounted(() => {
  void refreshAll();
});
</script>

<template>
  <div class="p-8 max-w-5xl mx-auto animation-fade-in w-full space-y-6">
    <div class="flex items-start justify-between gap-4">
      <div>
        <h2 class="text-2xl font-bold text-gray-900">{{ t("logs.title") }}</h2>
        <p class="text-gray-500 mt-2">{{ t("logs.subtitle") }}</p>
      </div>
      <button
        type="button"
        :disabled="loadingRecords"
        @click="refreshAll"
        class="bg-gray-900 hover:bg-black disabled:bg-gray-300 text-white px-4 py-2 rounded-lg text-sm"
      >
        {{ loadingRecords ? t("logs.refreshingRecords") : t("logs.refreshRecords") }}
      </button>
    </div>

    <div class="bg-white rounded-2xl border border-gray-200 shadow-sm p-5 space-y-4">
      <h3 class="text-lg font-semibold text-gray-800">{{ t("logs.recordsTitle") }}</h3>
      <p class="text-sm text-gray-500">{{ t("logs.recordsSubtitle") }}</p>

      <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
        <label class="block text-sm text-gray-700">
          {{ t("logs.appFilter") }}
          <select v-model="selectedApp" class="mt-2 w-full border border-gray-200 rounded-lg px-3 py-2 text-sm">
            <option value="all">{{ t("logs.appFilterAll") }}</option>
            <option v-for="app in appOptions" :key="app" :value="app">{{ app }}</option>
          </select>
        </label>

        <label class="block text-sm text-gray-700">
          {{ t("logs.operationType") }}
          <select v-model="operationType" class="mt-2 w-full border border-gray-200 rounded-lg px-3 py-2 text-sm">
            <option value="all">{{ t("logs.operationTypeAll") }}</option>
            <option value="migrate">{{ t("logs.operationTypeMigrate") }}</option>
            <option value="rollback">{{ t("logs.operationTypeRollback") }}</option>
          </select>
        </label>
      </div>

      <div v-if="timelineItems.length === 0 && !loadingRecords" class="text-sm text-gray-500">
        {{ t("logs.recordsEmpty") }}
      </div>

      <div class="space-y-3" v-else>
        <div
          v-for="item in timelineItems"
          :key="item.key"
          class="rounded-xl border border-gray-200 bg-gray-50/50 p-4 space-y-3"
        >
          <div class="flex items-start justify-between gap-3">
            <div class="min-w-0">
              <div class="text-sm text-gray-500">{{ item.appName }}</div>
              <div class="text-base font-semibold text-gray-800">{{ actionLabel(item.action) }}</div>
            </div>
            <span :class="['text-xs px-2 py-0.5 rounded-full whitespace-nowrap', resultClass(item.result)]">
              {{ resultLabel(item.result) }}
            </span>
          </div>

          <div class="text-xs text-gray-600 space-y-1">
            <div>{{ t("logs.timeRange") }}: {{ formatTime(item.startedAt) }} ~ {{ formatTime(item.endedAt) }}</div>
            <div>{{ t("logs.lastStep") }}: {{ item.lastStep }}</div>
          </div>

          <div v-if="item.result === 'failed'" class="space-y-2">
            <button
              type="button"
              class="text-red-600 hover:underline text-sm"
              @click="toggleFailedDetail(item.key)"
            >
              {{ expandedFailedKey === item.key ? t("logs.failureHide") : t("logs.failureShow") }}
            </button>

            <div
              v-if="expandedFailedKey === item.key"
              class="rounded-lg border border-red-200 bg-red-50 p-3 space-y-2"
            >
              <div class="text-sm font-semibold text-red-700">{{ t("logs.failureTitle") }}</div>
              <div
                v-for="failed in item.failedLogs.slice(-6)"
                :key="failed.log_id"
                class="text-xs text-red-700 space-y-1"
              >
                <div>
                  {{ formatTime(failed.created_at) }} · {{ failed.stage }}/{{ failed.step }}
                </div>
                <div>{{ t("logs.failureCode") }}: {{ failed.error_code || "UNKNOWN" }}</div>
                <div class="break-all">{{ failed.message || "-" }}</div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>

    <div v-if="error" class="rounded-xl border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-700">
      {{ error }}
    </div>
  </div>
</template>
