<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { AlertCircle, ArrowRightLeft, CheckCircle2, HardDrive } from "lucide-vue-next";
import type { AppScanResult, DiskStatus, MigrateRequest, RelocationResult } from "../types/contracts";
import { useI18n } from "../i18n";

interface SourceSummary {
  exists: boolean;
  hasSymlink: boolean;
  totalBytes: number;
  hasData: boolean;
}

interface MigrationPlanItem {
  app: AppScanResult;
  mode: MigrateRequest["mode"];
  execute: boolean;
  reason: string;
}

const props = defineProps<{
  showModal: boolean;
  selectedAppId: string;
  selectedApp: AppScanResult | null;
  disks: DiskStatus[];
}>();

const emit = defineEmits<{
  (e: "close"): void;
  (e: "done", message: string): void;
}>();
const { t } = useI18n();

const migrationStep = ref(0); // 0: setup, 1: migrating, 2: success
const progress = ref(0);
const targetDiskMount = ref("");
const targetRoot = ref("");
const allowExperimental = ref(false);
const cleanupBackupAfterMigrate = ref(true);
const loading = ref(false);
const selectingTargetRoot = ref(false);
const error = ref<string | null>(null);
const successResults = ref<RelocationResult[]>([]);

let progressTimer: number | null = null;

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

function summarizeSource(app?: AppScanResult): SourceSummary {
  const paths = app?.detected_paths ?? [];
  const existing = paths.filter((path) => path.exists);
  const hasSymlink = existing.some((path) => path.is_symlink);
  const totalBytes = existing.reduce((sum, path) => sum + path.size_bytes, 0);
  return {
    exists: existing.length > 0,
    hasSymlink,
    totalBytes,
    hasData: totalBytes > 0
  };
}

function pickMode(app: AppScanResult, source: SourceSummary): MigrateRequest["mode"] {
  if (source.hasData) {
    return "migrate";
  }
  if (app.allow_bootstrap_if_source_missing) {
    return "bootstrap";
  }
  return "migrate";
}

function buildPlanItem(app: AppScanResult): MigrationPlanItem {
  const source = summarizeSource(app);
  const mode = pickMode(app, source);

  if (app.tier === "blocked") {
    return { app, mode, execute: false, reason: t("migrationDialog.reason.blocked") };
  }
  if (app.running) {
    return { app, mode, execute: false, reason: t("migrationDialog.reason.running") };
  }
  if (source.hasSymlink) {
    return { app, mode, execute: false, reason: t("migrationDialog.reason.migrated") };
  }
  if (!source.exists && !app.allow_bootstrap_if_source_missing) {
    return { app, mode, execute: false, reason: t("migrationDialog.reason.sourceMissingNoData") };
  }
  if (!source.exists && mode === "bootstrap") {
    return { app, mode, execute: true, reason: t("migrationDialog.reason.sourceMissingBootstrap") };
  }
  if (!source.hasData && mode === "bootstrap") {
    return { app, mode, execute: true, reason: t("migrationDialog.reason.sourceEmptyBootstrap") };
  }
  return { app, mode, execute: true, reason: t("migrationDialog.reason.sourceDetected") };
}

const candidateDisks = computed(() =>
  props.disks.filter((disk) => disk.is_mounted && disk.is_writable)
);

const migrationPlan = computed<MigrationPlanItem[]>(() => {
  if (!props.selectedApp) {
    return [];
  }
  return [buildPlanItem(props.selectedApp)];
});

const executablePlan = computed(() => migrationPlan.value.filter((item) => item.execute));
const skippedPlan = computed(() => migrationPlan.value.filter((item) => !item.execute));
const needsExperimentalConfirm = computed(() =>
  executablePlan.value.some((item) => item.app.tier === "experimental")
);

const selectedSource = computed(() => summarizeSource(props.selectedApp ?? undefined));

const targetRootOptions = computed(() => {
  const options = targetDiskMount.value
    ? [
        {
          value: targetDiskMount.value,
          label: t("migrationDialog.targetRootDiskRoot", { root: targetDiskMount.value })
        },
        {
          value: `${targetDiskMount.value}/DataDock`,
          label: t("migrationDialog.targetRootRecommended", { root: targetDiskMount.value })
        },
        {
          value: `${targetDiskMount.value}/RelocatorData`,
          label: t("migrationDialog.targetRootRelocator", { root: targetDiskMount.value })
        }
      ]
    : [];

  if (targetRoot.value && !options.some((item) => item.value === targetRoot.value)) {
    options.unshift({
      value: targetRoot.value,
      label: t("migrationDialog.targetRootSystemPick", { root: targetRoot.value })
    });
  }
  return options;
});

const selectedDiskName = computed(() => {
  const disk = props.disks.find((item) => item.mount_point === targetDiskMount.value);
  return disk?.display_name ?? targetDiskMount.value;
});

const selectedAppSizeText = computed(() => formatBytes(selectedSource.value.totalBytes));

const canStart = computed(() => {
  if (!props.selectedApp) {
    return false;
  }
  if (!targetRoot.value.trim()) {
    return false;
  }
  if (loading.value) {
    return false;
  }
  if (executablePlan.value.length === 0) {
    return false;
  }
  if (needsExperimentalConfirm.value && !allowExperimental.value) {
    return false;
  }
  return true;
});

function resetDialogState(): void {
  migrationStep.value = 0;
  progress.value = 0;
  allowExperimental.value = false;
  cleanupBackupAfterMigrate.value = true;
  loading.value = false;
  error.value = null;
  successResults.value = [];

  const defaultDisk = candidateDisks.value[0]?.mount_point ?? "";
  targetDiskMount.value = defaultDisk;
  targetRoot.value = defaultDisk;
}

watch(
  () => [props.showModal, props.selectedAppId, candidateDisks.value.length],
  ([show]) => {
    if (show) {
      resetDialogState();
    }
  },
  { immediate: true }
);

function stopProgressAnimation(): void {
  if (progressTimer !== null) {
    window.clearInterval(progressTimer);
    progressTimer = null;
  }
}

function startProgressAnimation(): void {
  stopProgressAnimation();
  progressTimer = window.setInterval(() => {
    if (progress.value >= 92) {
      return;
    }
    const next = progress.value + Math.floor(Math.random() * 8) + 3;
    progress.value = Math.min(92, next);
  }, 380);
}

function onSelectDisk(mountPoint: string): void {
  targetDiskMount.value = mountPoint;
  targetRoot.value = mountPoint;
}

async function onPickTargetRoot(): Promise<void> {
  if (!targetDiskMount.value) {
    error.value = t("migrationDialog.selectDiskFirst");
    return;
  }

  selectingTargetRoot.value = true;
  try {
    const picked = await open({
      directory: true,
      multiple: false,
      defaultPath: targetRoot.value || targetDiskMount.value || "/Volumes",
      title: t("migrationDialog.pickTitle")
    });
    if (!picked || Array.isArray(picked)) {
      return;
    }

    const onSelectedDisk =
      picked === targetDiskMount.value || picked.startsWith(`${targetDiskMount.value}/`);
    if (!onSelectedDisk) {
      error.value = t("migrationDialog.pathNotInDisk", { disk: targetDiskMount.value });
      return;
    }

    targetRoot.value = picked;
    error.value = null;
  } catch (err) {
    error.value = String(err);
  } finally {
    selectingTargetRoot.value = false;
  }
}

async function onStartMigration(): Promise<void> {
  if (!canStart.value) {
    return;
  }

  loading.value = true;
  error.value = null;
  migrationStep.value = 1;
  progress.value = 5;
  startProgressAnimation();

  const results: RelocationResult[] = [];
  try {
    for (let index = 0; index < executablePlan.value.length; index += 1) {
      const item = executablePlan.value[index];
      const request: MigrateRequest = {
        app_id: item.app.app_id,
        target_root: targetRoot.value.trim(),
        mode: item.mode,
        allow_experimental: allowExperimental.value,
        cleanup_backup_after_migrate: cleanupBackupAfterMigrate.value
      };
      const result = await invoke<RelocationResult>("migrate_app", { req: request });
      results.push(result);

      const ratio = (index + 1) / executablePlan.value.length;
      progress.value = Math.max(progress.value, Math.floor(ratio * 96));
    }

    stopProgressAnimation();
    successResults.value = results;
    progress.value = 100;
    setTimeout(() => {
      migrationStep.value = 2;
    }, 200);
  } catch (err) {
    stopProgressAnimation();
    migrationStep.value = 0;
    error.value = t("migrationDialog.errors.startFailed", { error: String(err) });
  } finally {
    loading.value = false;
  }
}

function onFinish(): void {
  const names = successResults.value.map((item) => item.app_id).join(" / ");
  const label =
    names.length > 0 ? names : props.selectedApp?.display_name ?? t("migrationDialog.fallbackLabel");
  emit("done", t("migrationDialog.doneMessage", { label }));
}

onBeforeUnmount(() => {
  stopProgressAnimation();
});
</script>

<template>
  <div
    v-if="props.showModal && props.selectedApp"
    class="fixed inset-0 bg-black/40 backdrop-blur-sm flex items-center justify-center z-50"
  >
    <div class="bg-white rounded-2xl shadow-2xl w-full max-w-lg overflow-hidden flex flex-col">
      <template v-if="migrationStep === 0">
        <div class="p-6 border-b border-gray-100 flex items-start gap-4">
          <div class="w-12 h-12 bg-blue-50 text-blue-500 rounded-full flex items-center justify-center flex-shrink-0">
            <ArrowRightLeft :size="24" />
          </div>
          <div>
            <h2 class="text-xl font-bold text-gray-900">
              {{ t("migrationDialog.title", { name: props.selectedApp.display_name }) }}
            </h2>
            <p class="text-sm text-gray-500 mt-1">{{ t("migrationDialog.subtitle") }}</p>
          </div>
        </div>

        <div class="p-6 bg-gray-50/50 space-y-4">
          <div>
            <label class="block text-sm font-medium text-gray-700 mb-2">{{
              t("migrationDialog.sizeLabel")
            }}</label>
            <div class="bg-white p-3 rounded-lg border border-gray-200 text-lg font-mono font-bold text-gray-800">
              {{ selectedAppSizeText }}
            </div>
          </div>

          <div>
            <label class="block text-sm font-medium text-gray-700 mb-2">{{
              t("migrationDialog.diskLabel")
            }}</label>
            <p class="text-xs text-gray-500 mb-2">{{ t("migrationDialog.diskHint") }}</p>
            <div class="space-y-2">
              <label
                v-for="disk in candidateDisks"
                :key="disk.mount_point"
                :class="`flex items-center justify-between p-3 rounded-lg border cursor-pointer transition-colors ${targetDiskMount === disk.mount_point ? 'border-blue-500 bg-blue-50/30' : 'border-gray-200 bg-white hover:border-blue-300'}`"
              >
                <div class="flex items-center gap-3">
                  <input
                    type="radio"
                    name="targetDisk"
                    :value="disk.mount_point"
                    :checked="targetDiskMount === disk.mount_point"
                    @change="onSelectDisk(disk.mount_point)"
                    class="w-4 h-4 text-blue-600 focus:ring-blue-500"
                  />
                  <HardDrive :class="targetDiskMount === disk.mount_point ? 'text-blue-500' : 'text-gray-400'" :size="20" />
                  <span class="font-medium text-gray-800">{{ disk.display_name }}</span>
                </div>
                <span class="text-sm text-gray-500">{{
                  t("migrationDialog.diskFree", { size: formatBytes(disk.free_bytes) })
                }}</span>
              </label>
            </div>
            <p v-if="candidateDisks.length === 0" class="mt-2 text-xs text-red-600">
              {{ t("migrationDialog.diskNone") }}
            </p>
          </div>

          <div>
            <label class="block text-sm font-medium text-gray-700 mb-2">{{
              t("migrationDialog.targetRootLabel")
            }}</label>
            <div class="flex items-center gap-2">
              <select
                class="flex-1 h-10 border border-gray-200 rounded-lg px-3 text-sm"
                :value="targetRoot"
                @change="targetRoot = ($event.target as HTMLSelectElement).value"
              >
                <option v-for="option in targetRootOptions" :key="option.value" :value="option.value">
                  {{ option.label }}
                </option>
              </select>
              <button
                type="button"
                :disabled="loading || selectingTargetRoot"
                @click="onPickTargetRoot"
                class="h-10 px-3 inline-flex items-center border border-gray-200 rounded-lg text-sm text-gray-700 hover:bg-gray-100 whitespace-nowrap"
              >
                {{ selectingTargetRoot ? t("migrationDialog.picking") : t("migrationDialog.pickButton") }}
              </button>
            </div>
          </div>

          <label v-if="needsExperimentalConfirm" class="flex items-start gap-2 text-sm text-gray-700">
            <input v-model="allowExperimental" type="checkbox" class="mt-0.5" />
            <span>{{ t("migrationDialog.allowExperimental") }}</span>
          </label>

          <label class="flex items-start gap-2 text-sm text-gray-700">
            <input v-model="cleanupBackupAfterMigrate" type="checkbox" class="mt-0.5" />
            <span>{{ t("migrationDialog.cleanupBackup") }}</span>
          </label>

          <div class="bg-yellow-50 p-3 rounded-lg flex items-start gap-2 border border-yellow-100">
            <AlertCircle :size="16" class="text-yellow-600 mt-0.5" />
            <p class="text-xs text-yellow-700 leading-relaxed">
              {{ t("migrationDialog.warning", { name: props.selectedApp.display_name }) }}
            </p>
          </div>

          <div v-if="skippedPlan.length > 0" class="text-xs text-gray-500 leading-5">
            <div class="font-medium text-gray-700">{{ t("migrationDialog.skippedTitle") }}</div>
            <div v-for="item in skippedPlan" :key="item.app.app_id">{{ item.app.display_name }}：{{ item.reason }}</div>
          </div>

          <div v-if="error" class="rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700">
            {{ error }}
          </div>
        </div>

        <div class="p-4 border-t border-gray-100 flex justify-end gap-3 bg-white">
          <button
            type="button"
            :disabled="loading"
            @click="emit('close')"
            class="px-4 py-2 text-gray-600 hover:bg-gray-100 rounded-lg font-medium transition-colors"
          >
            {{ t("migrationDialog.cancel") }}
          </button>
          <button
            type="button"
            :disabled="!canStart"
            @click="onStartMigration"
            class="px-6 py-2 bg-blue-500 hover:bg-blue-600 disabled:bg-gray-300 text-white rounded-lg font-medium shadow-sm shadow-blue-500/30 transition-colors"
          >
            {{ loading ? t("migrationDialog.migratingBtn") : t("migrationDialog.start") }}
          </button>
        </div>
      </template>

      <div v-if="migrationStep === 1" class="p-10 flex flex-col items-center justify-center text-center">
        <div class="w-16 h-16 border-4 border-gray-100 border-t-blue-500 rounded-full animate-spin mb-6"></div>
        <h2 class="text-xl font-bold text-gray-900 mb-2">{{ t("migrationDialog.migratingTitle") }}</h2>
        <p class="text-sm text-gray-500 mb-6">
          {{ t("migrationDialog.migratingSubtitle", { count: executablePlan.length, disk: selectedDiskName }) }}
        </p>

        <div class="w-full bg-gray-100 rounded-full h-3 mb-2 overflow-hidden">
          <div class="bg-blue-500 h-full transition-all duration-300 ease-out" :style="{ width: `${progress}%` }"></div>
        </div>
        <div class="w-full flex justify-between text-xs font-mono text-gray-400">
          <span>{{ progress }}%</span>
          <span>{{ t("migrationDialog.keepDiskOnline") }}</span>
        </div>
      </div>

      <div v-if="migrationStep === 2" class="p-10 flex flex-col items-center justify-center text-center">
        <div class="w-16 h-16 bg-green-100 text-green-500 rounded-full flex items-center justify-center mb-6">
          <CheckCircle2 :size="32" />
        </div>
        <h2 class="text-xl font-bold text-gray-900 mb-2">{{ t("migrationDialog.successTitle") }}</h2>
        <p class="text-sm text-gray-500 mb-8">
          {{
            t("migrationDialog.successText", {
              name: props.selectedApp.display_name,
              disk: selectedDiskName,
              count: successResults.length
            })
          }}
        </p>

        <button
          type="button"
          @click="onFinish"
          class="w-full py-3 bg-gray-900 hover:bg-black text-white rounded-xl font-medium transition-colors"
        >
          {{ t("migrationDialog.finish") }}
        </button>
      </div>
    </div>
  </div>
</template>
