<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { computed, onMounted, ref } from "vue";
import type { AppScanResult, DiskStatus, MigrateRequest, RelocationResult } from "../types/contracts";

const WECHAT_BUNDLE_IDS = [
  "wechat-non-mas",
  "wechat-file-provider-extension",
  "wechat-mac-share-extension"
] as const;

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

const loading = ref(false);
const selectingTargetRoot = ref(false);
const apps = ref<AppScanResult[]>([]);
const selectedAppId = ref("");
const disks = ref<DiskStatus[]>([]);
const selectedDiskMount = ref("");
const targetRoot = ref("");
const allowExperimental = ref(false);
const cleanupBackupAfterMigrate = ref(true);
const payload = ref<Record<string, unknown> | null>(null);
const error = ref<string | null>(null);

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
    return { app, mode, execute: false, reason: "blocked，不允许迁移" };
  }
  if (app.running) {
    return { app, mode, execute: false, reason: "应用运行中，请先退出" };
  }
  if (source.hasSymlink) {
    return { app, mode, execute: false, reason: "源目录已是软链接，已迁移" };
  }
  if (!source.exists && !app.allow_bootstrap_if_source_missing) {
    return { app, mode, execute: false, reason: "源目录不存在，且该画像不允许 bootstrap" };
  }
  if (!source.exists && mode === "bootstrap") {
    return { app, mode, execute: true, reason: "未检测到源目录，执行首次引导 bootstrap" };
  }
  if (!source.hasData && mode === "bootstrap") {
    return { app, mode, execute: true, reason: "源目录无数据，执行首次引导 bootstrap" };
  }
  return { app, mode, execute: true, reason: "检测到源数据，执行 migrate" };
}

const selectedApp = computed(() => apps.value.find((item) => item.app_id === selectedAppId.value));
const selectedSourceSummary = computed(() => summarizeSource(selectedApp.value));
const isWechatBundleSelection = computed(() => selectedAppId.value === "wechat-non-mas");
const migrationPlan = computed<MigrationPlanItem[]>(() => {
  const app = selectedApp.value;
  if (!app) {
    return [];
  }
  if (!isWechatBundleSelection.value) {
    return [buildPlanItem(app)];
  }
  return WECHAT_BUNDLE_IDS.map((appId) => apps.value.find((item) => item.app_id === appId))
    .filter((item): item is AppScanResult => Boolean(item))
    .map((item) => buildPlanItem(item));
});
const executablePlan = computed(() => migrationPlan.value.filter((item) => item.execute));
const skippedPlan = computed(() => migrationPlan.value.filter((item) => !item.execute));
const needsExperimentalConfirm = computed(() =>
  executablePlan.value.some((item) => item.app.tier === "experimental")
);
const autoModeLabel = computed(() =>
  !selectedApp.value
    ? "待判定"
    : isWechatBundleSelection.value
      ? "bundle（微信主容器 + 扩展联动）"
      : pickMode(selectedApp.value, selectedSourceSummary.value) === "bootstrap"
        ? "bootstrap（首次引导）"
        : "migrate（已有数据迁移）"
);
const autoModeReason = computed(() => {
  if (!selectedApp.value) {
    return "请选择应用后自动判定。";
  }
  if (isWechatBundleSelection.value) {
    return `微信容器联动计划：执行 ${executablePlan.value.length} 个，跳过 ${skippedPlan.value.length} 个。`;
  }
  if (selectedSourceSummary.value.hasData) {
    return "检测到源目录已有数据，自动使用 migrate。";
  }
  if (selectedApp.value.allow_bootstrap_if_source_missing) {
    return "未检测到源目录数据，且该画像允许首次引导，自动使用 bootstrap。";
  }
  return "该画像不允许 bootstrap，将保持 migrate。";
});
const planPreview = computed(() =>
  migrationPlan.value.map((item) => ({
    app_id: item.app.app_id,
    display_name: item.app.display_name,
    mode: item.mode,
    execute: item.execute,
    reason: item.reason
  }))
);
const targetRootOptions = computed(() => {
  const options = selectedDiskMount.value
    ? [
        {
          value: selectedDiskMount.value,
          label: `${selectedDiskMount.value}（盘根目录）`
        },
        {
          value: `${selectedDiskMount.value}/DataDock`,
          label: `${selectedDiskMount.value}/DataDock（推荐独立目录）`
        },
        {
          value: `${selectedDiskMount.value}/RelocatorData`,
          label: `${selectedDiskMount.value}/RelocatorData`
        }
      ]
    : [];

  if (targetRoot.value && !options.some((item) => item.value === targetRoot.value)) {
    options.unshift({
      value: targetRoot.value,
      label: `${targetRoot.value}（系统选择）`
    });
  }

  return options;
});
const submitBlockReason = computed(() => {
  if (!selectedAppId.value) {
    return "未检测到可迁移应用。";
  }
  if (!targetRoot.value.trim()) {
    return "请先选择目标盘。";
  }
  if (executablePlan.value.length === 0) {
    return "没有可执行迁移项（可能都已迁移或源目录尚未初始化）。";
  }
  if (needsExperimentalConfirm.value && !allowExperimental.value) {
    return "该应用属于 experimental，请先勾选风险确认。";
  }
  return "";
});
const canSubmit = computed(() => submitBlockReason.value.length === 0);

async function loadApps(): Promise<void> {
  try {
    apps.value = await invoke<AppScanResult[]>("scan_apps");
    if (apps.value.length === 0) {
      selectedAppId.value = "";
      allowExperimental.value = false;
      return;
    }
    const selectedExists = apps.value.some((item) => item.app_id === selectedAppId.value);
    if (!selectedExists) {
      const preferred = apps.value.find((item) => item.tier !== "blocked") ?? apps.value[0];
      selectedAppId.value = preferred.app_id;
      allowExperimental.value = preferred.tier === "experimental" ? allowExperimental.value : false;
    }
  } catch (err) {
    error.value = String(err);
  }
}

async function loadDisks(): Promise<void> {
  try {
    disks.value = await invoke<DiskStatus[]>("get_disk_status");
    const diskExists = disks.value.some((item) => item.mount_point === selectedDiskMount.value);
    if (!selectedDiskMount.value || !diskExists) {
      selectedDiskMount.value = disks.value[0]?.mount_point ?? "";
    }
    if (!targetRoot.value) {
      targetRoot.value = targetRootOptions.value[0]?.value ?? "";
    }
  } catch (err) {
    error.value = String(err);
  }
}

function onSelectApp(appId: string): void {
  selectedAppId.value = appId;
  const app = apps.value.find((item) => item.app_id === appId);
  if (!app || app.tier !== "experimental") {
    allowExperimental.value = false;
  }
}

function onSelectDisk(mountPoint: string): void {
  selectedDiskMount.value = mountPoint;
  targetRoot.value = targetRootOptions.value[0]?.value ?? "";
}

function onSelectTargetRoot(path: string): void {
  targetRoot.value = path;
}

async function onPickTargetRoot(): Promise<void> {
  if (!selectedDiskMount.value) {
    error.value = "请先选择目标盘，再使用系统选择。";
    return;
  }

  selectingTargetRoot.value = true;
  try {
    const picked = await open({
      directory: true,
      multiple: false,
      defaultPath: targetRoot.value || selectedDiskMount.value || "/Volumes",
      title: "选择目标盘根路径"
    });
    if (!picked || Array.isArray(picked)) {
      return;
    }

    const onSelectedDisk =
      picked === selectedDiskMount.value || picked.startsWith(`${selectedDiskMount.value}/`);
    if (!onSelectedDisk) {
      error.value = `所选路径不在目标盘 ${selectedDiskMount.value} 下，请在该盘内选择。`;
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

function diskLabel(disk: DiskStatus): string {
  const freeGb = disk.free_bytes > 0 ? `${(disk.free_bytes / 1024 / 1024 / 1024).toFixed(1)}GB 可用` : "可用空间未知";
  return `${disk.display_name} (${disk.mount_point}) · ${freeGb}`;
}

async function onMigrate(): Promise<void> {
  if (!canSubmit.value) {
    return;
  }
  loading.value = true;
  error.value = null;
  payload.value = null;

  const results: RelocationResult[] = [];
  try {
    for (const item of executablePlan.value) {
      const request: MigrateRequest = {
        app_id: item.app.app_id,
        target_root: targetRoot.value.trim(),
        mode: item.mode,
        allow_experimental: allowExperimental.value,
        cleanup_backup_after_migrate: cleanupBackupAfterMigrate.value
      };
      const result = await invoke<RelocationResult>("migrate_app", { req: request });
      results.push(result);
    }
    payload.value = {
      strategy: isWechatBundleSelection.value ? "wechat_bundle" : "single",
      executed_count: results.length,
      skipped_count: skippedPlan.value.length,
      results,
      skipped: skippedPlan.value.map((item) => ({
        app_id: item.app.app_id,
        display_name: item.app.display_name,
        reason: item.reason
      }))
    };
  } catch (err) {
    error.value = `迁移失败：${String(err)}`;
    payload.value = {
      strategy: isWechatBundleSelection.value ? "wechat_bundle" : "single",
      executed_count: results.length,
      executed: results.map((item) => ({
        app_id: item.app_id,
        relocation_id: item.relocation_id,
        state: item.state
      })),
      pending: executablePlan.value
        .map((item) => item.app.app_id)
        .filter((appId) => !results.some((item) => item.app_id === appId))
    };
  } finally {
    await loadApps();
    loading.value = false;
  }
}

onMounted(() => {
  void loadApps();
  void loadDisks();
});
</script>

<template>
  <section class="panel">
    <h2>迁移执行</h2>
    <p class="muted">
      选择应用和目标盘后直接执行迁移，模式由系统自动判定。选择微信主容器时将联动迁移微信扩展容器。
    </p>
    <label class="field">
      应用
      <select :value="selectedAppId" @change="onSelectApp(($event.target as HTMLSelectElement).value)">
        <option v-for="app in apps" :key="app.app_id" :value="app.app_id">
          {{ app.display_name }} ({{ app.tier }})
        </option>
      </select>
    </label>
    <label class="field">
      目标盘（自动扫描）
      <select :value="selectedDiskMount" @change="onSelectDisk(($event.target as HTMLSelectElement).value)">
        <option value="" disabled>请选择目标盘</option>
        <option v-for="disk in disks" :key="disk.mount_point" :value="disk.mount_point">
          {{ diskLabel(disk) }}
        </option>
      </select>
    </label>
    <label class="field">
      目标盘根路径（下拉选择）
      <div style="display: flex; gap: 8px; align-items: center; flex-wrap: wrap">
        <select :value="targetRoot" @change="onSelectTargetRoot(($event.target as HTMLSelectElement).value)">
          <option value="" disabled>请选择目标根路径</option>
          <option v-for="item in targetRootOptions" :key="item.value" :value="item.value">
            {{ item.label }}
          </option>
        </select>
        <button type="button" :disabled="loading || selectingTargetRoot" @click="onPickTargetRoot">
          {{ selectingTargetRoot ? "选择中..." : "系统选择..." }}
        </button>
      </div>
    </label>
    <label class="field">
      自动迁移模式（只读）
      <input :value="autoModeLabel" type="text" readonly />
      <span class="muted">{{ autoModeReason }}</span>
    </label>
    <label class="field">
      执行计划（只读）
      <pre>{{ JSON.stringify(planPreview, null, 2) }}</pre>
    </label>
    <label v-if="needsExperimentalConfirm" class="field">
      <span>Experimental 画像确认</span>
      <div style="display: flex; align-items: center; gap: 8px">
        <input id="allow-experimental" v-model="allowExperimental" type="checkbox" />
        <label for="allow-experimental">我已知晓风险并允许 experimental 迁移</label>
      </div>
    </label>
    <label class="field">
      <span>迁移后释放本地空间</span>
      <div style="display: flex; align-items: center; gap: 8px">
        <input id="cleanup-backup" v-model="cleanupBackupAfterMigrate" type="checkbox" />
        <label for="cleanup-backup">
          迁移成功后清理本地备份（.bak）。若后续回滚，将从目标目录恢复到本地。
        </label>
      </div>
    </label>
    <p v-if="submitBlockReason" class="muted">{{ submitBlockReason }}</p>
    <div style="display: flex; gap: 8px; margin-top: 8px; flex-wrap: wrap">
      <button type="button" :disabled="loading || !canSubmit" @click="onMigrate">
        {{ loading ? "迁移中..." : "执行迁移" }}
      </button>
      <button type="button" :disabled="loading" @click="loadApps">刷新应用列表</button>
      <button type="button" :disabled="loading" @click="loadDisks">刷新目标盘</button>
    </div>
    <pre v-if="payload">{{ JSON.stringify(payload, null, 2) }}</pre>
    <pre v-else-if="error">{{ error }}</pre>
  </section>
</template>
