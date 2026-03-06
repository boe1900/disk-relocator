<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { computed, onMounted, ref } from "vue";
import type { AppScanResult } from "../types/contracts";

const loading = ref(false);
const payload = ref<AppScanResult[]>([]);
const error = ref<string | null>(null);

async function onScan(): Promise<void> {
  loading.value = true;
  error.value = null;
  try {
    payload.value = await invoke<AppScanResult[]>("scan_apps");
  } catch (err) {
    error.value = String(err);
  } finally {
    loading.value = false;
  }
}

const totalCount = computed(() => payload.value.length);
const migratableCount = computed(() =>
  payload.value.filter((item) => item.tier !== "blocked" && !item.running).length
);
const blockedCount = computed(() => payload.value.filter((item) => item.tier === "blocked").length);

function migrationHint(item: AppScanResult): string {
  if (item.tier === "blocked") {
    return "当前版本不支持迁移";
  }
  if (item.running) {
    return "应用正在运行，请先退出";
  }
  if (item.tier === "experimental") {
    return "可迁移（实验支持，迁移时需二次确认）";
  }
  return "可迁移";
}

onMounted(() => {
  void onScan();
});
</script>

<template>
  <section class="panel">
    <h2>应用扫描</h2>
    <p class="muted">启动后自动扫描；你也可以手动刷新。</p>
    <div style="display: flex; gap: 8px; margin-top: 8px; flex-wrap: wrap">
      <button type="button" :disabled="loading" @click="onScan">
        {{ loading ? "扫描中..." : "刷新扫描" }}
      </button>
      <span class="muted">共 {{ totalCount }} 个，可迁移 {{ migratableCount }} 个，不可迁移 {{ blockedCount }} 个</span>
    </div>
    <div v-if="payload.length > 0" class="scan-list">
      <article v-for="item in payload" :key="item.app_id" class="scan-item">
        <header class="scan-head">
          <strong>{{ item.display_name }}</strong>
          <span :class="item.tier === 'blocked' ? 'badge-blocked' : 'badge-ok'">
            {{ item.tier === "blocked" ? "不可迁移" : "可迁移" }}
          </span>
        </header>
        <p class="muted">{{ migrationHint(item) }}</p>
        <p class="muted">profile: {{ item.app_id }} · tier: {{ item.tier }}</p>
      </article>
    </div>
    <p v-else class="muted">未检测到可识别应用。请先启动一次目标应用后再刷新扫描。</p>
    <pre v-if="error">{{ error }}</pre>
  </section>
</template>
