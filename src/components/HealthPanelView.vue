<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { onBeforeUnmount, onMounted, computed, ref, watch } from "vue";
import { AlertCircle, CheckCircle2, ChevronLeft, ChevronRight, HardDrive } from "lucide-vue-next";
import { useI18n } from "../i18n";
import { formatCommandError } from "../utils/error";
import type {
  DiskStatus,
  HealthEvent,
  HealthEventsRequest,
  HealthStatus,
  ReconcileRequest,
  ReconcileResult,
  RollbackRequest
} from "../types/contracts";

const diskPayload = ref<DiskStatus[]>([]);
const healthPayload = ref<HealthStatus[]>([]);
const historyPayload = ref<HealthEvent[]>([]);
const rollbackingRelocationId = ref<string | null>(null);
const selfChecking = ref(false);
const loading = ref(false);
const error = ref<string | null>(null);
const info = ref<string | null>(null);
const mountedDiskIndex = ref(0);
const { t } = useI18n();
let timer: number | undefined;

function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) {
    return "0 B";
  }
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  return `${value.toFixed(unitIndex === 0 ? 0 : 1)} ${units[unitIndex]}`;
}

const healthyCount = computed(() =>
  healthPayload.value.filter((status) => status.state === "healthy").length
);
const unhealthyCount = computed(() =>
  healthPayload.value.filter((status) => status.state !== "healthy").length
);
const mountedDisks = computed(() =>
  diskPayload.value.filter((disk) => disk.is_mounted && disk.is_writable)
);
const currentMountedDisk = computed(() => {
  if (mountedDisks.value.length === 0) {
    return null;
  }
  const index =
    ((mountedDiskIndex.value % mountedDisks.value.length) + mountedDisks.value.length) %
    mountedDisks.value.length;
  return mountedDisks.value[index];
});
const mountedDiskPosition = computed(() => {
  if (mountedDisks.value.length === 0) {
    return 0;
  }
  return (
    ((mountedDiskIndex.value % mountedDisks.value.length) + mountedDisks.value.length) %
      mountedDisks.value.length +
    1
  );
});

function healthLabel(state: HealthStatus["state"]): string {
  if (state === "healthy") {
    return t("health.state.healthy");
  }
  if (state === "degraded") {
    return t("health.state.degraded");
  }
  return t("health.state.broken");
}

function diskState(status: HealthStatus): string {
  const code = status.checks[0]?.code ?? "";
  if (code.includes("DISK_OFFLINE")) {
    return t("health.state.unmounted");
  }
  if (code.includes("TARGET_READONLY")) {
    return t("health.state.readonly");
  }
  return t("health.state.mounted");
}

function prevMountedDisk(): void {
  if (mountedDisks.value.length <= 1) {
    return;
  }
  mountedDiskIndex.value =
    (mountedDiskIndex.value - 1 + mountedDisks.value.length) % mountedDisks.value.length;
}

function nextMountedDisk(): void {
  if (mountedDisks.value.length <= 1) {
    return;
  }
  mountedDiskIndex.value = (mountedDiskIndex.value + 1) % mountedDisks.value.length;
}

async function refreshPanel(): Promise<void> {
  loading.value = true;
  error.value = null;
  try {
    const req: HealthEventsRequest = { limit: 30 };
    const [disk, health, history] = await Promise.all([
      invoke<DiskStatus[]>("get_disk_status"),
      invoke<HealthStatus[]>("check_health"),
      invoke<HealthEvent[]>("list_health_events", { req })
    ]);
    diskPayload.value = disk;
    healthPayload.value = health;
    historyPayload.value = history;
  } catch (err) {
    error.value = t("health.errorRefreshFailed", { error: formatCommandError(err) });
  } finally {
    loading.value = false;
  }
}

async function runReconcile(applySafeFixes: boolean): Promise<ReconcileResult> {
  const req: ReconcileRequest = {
    apply_safe_fixes: applySafeFixes,
    limit: 500
  };
  return invoke<ReconcileResult>("reconcile_relocations", { req });
}

async function onSelfCheck(): Promise<void> {
  selfChecking.value = true;
  error.value = null;
  try {
    const result = await runReconcile(true);
    await refreshPanel();
    const remaining = result.issues.filter((issue) => !issue.safe_fix_applied).length;
    info.value = t("health.infoSelfCheckDone", {
      drift: result.drift_count,
      fixed: result.fixed_count,
      remaining
    });
  } catch (err) {
    error.value = t("health.errorSelfCheckFailed", { error: formatCommandError(err) });
  } finally {
    selfChecking.value = false;
  }
}

async function onRollback(relocationId: string): Promise<void> {
  info.value = null;
  error.value = null;
  rollbackingRelocationId.value = relocationId;
  const req: RollbackRequest = { relocation_id: relocationId, force: true };
  try {
    await invoke("rollback_relocation", { req });
    info.value = t("health.infoRollbackDone", { id: relocationId });
    await refreshPanel();
  } catch (err) {
    error.value = t("health.errorRollbackFailed", { error: formatCommandError(err) });
  } finally {
    rollbackingRelocationId.value = null;
  }
}

onMounted(async () => {
  await refreshPanel();
  try {
    await runReconcile(true);
    await refreshPanel();
  } catch {
    // Keep health panel available even if reconcile scan fails on initial load.
  }
  timer = window.setInterval(() => {
    void refreshPanel();
  }, 10000);
});

watch(
  () => mountedDisks.value.length,
  (nextLength) => {
    if (nextLength === 0) {
      mountedDiskIndex.value = 0;
      return;
    }
    if (mountedDiskIndex.value >= nextLength) {
      mountedDiskIndex.value = nextLength - 1;
    }
    if (mountedDiskIndex.value < 0) {
      mountedDiskIndex.value = 0;
    }
  }
);

onBeforeUnmount(() => {
  if (timer !== undefined) {
    window.clearInterval(timer);
  }
});
</script>

<template>
  <div class="p-8 max-w-5xl mx-auto animation-fade-in w-full">
    <div class="mb-8 flex items-start justify-between gap-4">
      <div>
        <h2 class="text-2xl font-bold text-gray-900">{{ t("health.title") }}</h2>
        <p class="text-gray-500 mt-2">{{ t("health.subtitle") }}</p>
      </div>
      <button
        type="button"
        :disabled="loading || selfChecking"
        @click="onSelfCheck"
        class="bg-gray-900 hover:bg-black text-white px-4 py-2 rounded-lg text-sm"
      >
        {{ selfChecking ? t("health.checking") : t("health.checkNow") }}
      </button>
    </div>

    <div class="grid grid-cols-2 gap-6 mb-8">
      <div class="bg-white p-6 rounded-2xl border border-green-200 bg-green-50/30">
        <div class="flex items-center gap-3 mb-2">
          <CheckCircle2 class="text-green-500" :size="24" />
          <h3 class="text-lg font-semibold text-green-900">
            {{
              unhealthyCount === 0
                ? t("health.allHealthy")
                : t("health.issueCount", { count: unhealthyCount })
            }}
          </h3>
        </div>
        <p class="text-sm text-green-700">
          {{ t("health.summary", { total: healthPayload.length, healthy: healthyCount }) }}
        </p>
      </div>

      <div class="bg-white p-6 rounded-2xl border border-gray-200 shadow-sm">
        <div class="flex items-center justify-between gap-3 mb-2">
          <div class="flex items-center gap-3 min-w-0">
            <HardDrive class="text-blue-500 flex-shrink-0" :size="24" />
            <h3 class="text-lg font-semibold text-gray-800 truncate">
              {{
                mountedDisks.length > 0
                  ? t("health.disksOnlineCount", { count: mountedDisks.length })
                  : t("health.noDisk")
              }}
            </h3>
          </div>
          <div v-if="mountedDisks.length > 1" class="flex items-center gap-1 text-xs text-gray-500">
            <button
              type="button"
              class="p-1 rounded border border-gray-200 hover:bg-gray-100"
              @click="prevMountedDisk"
            >
              <ChevronLeft :size="14" />
            </button>
            <span class="font-mono">{{ mountedDiskPosition }} / {{ mountedDisks.length }}</span>
            <button
              type="button"
              class="p-1 rounded border border-gray-200 hover:bg-gray-100"
              @click="nextMountedDisk"
            >
              <ChevronRight :size="14" />
            </button>
          </div>
        </div>
        <div class="text-xs text-gray-500 mb-2">{{ t("health.diskFilterHint") }}</div>
        <div v-if="currentMountedDisk" class="space-y-1.5">
          <div class="text-sm text-gray-700 truncate">{{ currentMountedDisk.display_name }}</div>
          <div class="text-xs text-gray-500 font-mono truncate">{{ currentMountedDisk.mount_point }}</div>
          <div class="text-sm text-gray-500">
            {{
              t("health.diskCapacity", {
                free: formatBytes(currentMountedDisk.free_bytes),
                total: formatBytes(currentMountedDisk.total_bytes)
              })
            }}
          </div>
        </div>
        <p class="text-sm text-gray-500" v-else>{{ t("health.mountPointHint") }}</p>
      </div>
    </div>

    <h3 class="text-lg font-semibold text-gray-800 mb-4">{{ t("health.tableTitle") }}</h3>
    <div class="bg-white rounded-2xl border border-gray-200 shadow-sm overflow-hidden">
      <table class="w-full text-left border-collapse">
        <thead>
          <tr class="bg-gray-50 border-b border-gray-200 text-sm text-gray-500">
            <th class="p-4 font-medium">{{ t("health.table.app") }}</th>
            <th class="p-4 font-medium">{{ t("health.table.link") }}</th>
            <th class="p-4 font-medium">{{ t("health.table.disk") }}</th>
            <th class="p-4 font-medium">{{ t("health.table.action") }}</th>
          </tr>
        </thead>
        <tbody class="text-sm">
          <tr v-for="status in healthPayload" :key="status.relocation_id" class="border-b border-gray-100 last:border-0">
            <td class="p-4">
              <div class="font-medium text-gray-800">{{ status.app_id }}</div>
              <div class="text-xs text-gray-400 font-mono mt-1">{{ status.relocation_id }}</div>
            </td>
            <td class="p-4">
              <span v-if="status.state === 'healthy'" class="text-green-600 flex items-center gap-1">
                <CheckCircle2 :size="14" /> {{ healthLabel(status.state) }}
              </span>
              <span v-else class="text-red-500 flex items-center gap-1">
                <AlertCircle :size="14" /> {{ healthLabel(status.state) }}
              </span>
            </td>
            <td
              class="p-4"
              :class="
                diskState(status) === t('health.state.mounted') ? 'text-gray-700' : 'text-red-500'
              "
            >
              {{ diskState(status) }}
            </td>
            <td class="p-4 flex gap-3">
              <button class="text-blue-500 hover:underline" @click="onSelfCheck">
                {{ t("health.recheck") }}
              </button>
              <button
                v-if="status.state !== 'healthy'"
                class="text-red-500 hover:underline"
                :disabled="rollbackingRelocationId === status.relocation_id"
                @click="onRollback(status.relocation_id)"
              >
                {{
                  rollbackingRelocationId === status.relocation_id
                    ? t("health.rollbacking")
                    : t("health.rollback")
                }}
              </button>
            </td>
          </tr>
          <tr v-if="!loading && healthPayload.length === 0">
            <td colspan="4" class="p-6 text-center text-sm text-gray-500">{{ t("health.empty") }}</td>
          </tr>
        </tbody>
      </table>
    </div>

    <div v-if="historyPayload.length > 0" class="mt-6 rounded-2xl border border-gray-200 bg-white p-4">
      <h4 class="text-sm font-semibold text-gray-800 mb-2">{{ t("health.recentEvents") }}</h4>
      <div class="space-y-2 text-xs text-gray-600">
        <div v-for="item in historyPayload.slice(0, 6)" :key="item.snapshot_id">
          {{ item.observed_at }} · {{ item.app_id }} · {{ item.check_code }} · {{ item.state }}
        </div>
      </div>
    </div>

    <div v-if="info" class="mt-4 rounded-xl border border-green-200 bg-green-50 px-4 py-3 text-sm text-green-700">
      {{ info }}
    </div>
    <div v-if="error" class="mt-4 rounded-xl border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-700">
      {{ error }}
    </div>
  </div>
</template>
