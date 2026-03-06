<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { computed, onMounted, ref } from "vue";
import type { RelocationSummary, RelocationResult, RollbackRequest } from "../types/contracts";

const listPayload = ref<RelocationSummary[]>([]);
const selectedRelocationId = ref("");
const forceRollback = ref(true);
const loading = ref(false);
const rollbackPayload = ref<RelocationResult | null>(null);
const error = ref<string | null>(null);

async function onList(): Promise<void> {
  loading.value = true;
  error.value = null;
  try {
    listPayload.value = await invoke<RelocationSummary[]>("list_relocations");
    if (!selectedRelocationId.value && listPayload.value.length > 0) {
      selectedRelocationId.value = listPayload.value[0].relocation_id;
    }
  } catch (err) {
    error.value = String(err);
  } finally {
    loading.value = false;
  }
}

async function onRollback(): Promise<void> {
  if (!selectedRelocationId.value) {
    return;
  }
  loading.value = true;
  error.value = null;
  const req: RollbackRequest = {
    relocation_id: selectedRelocationId.value,
    force: forceRollback.value
  };
  try {
    rollbackPayload.value = await invoke<RelocationResult>("rollback_relocation", { req });
    await onList();
  } catch (err) {
    error.value = String(err);
  } finally {
    loading.value = false;
  }
}

const selectedSummary = computed(() =>
  listPayload.value.find((item) => item.relocation_id === selectedRelocationId.value)
);

onMounted(() => {
  void onList();
});
</script>

<template>
  <section class="panel">
    <h2>迁移状态与回滚</h2>
    <p class="muted">查询迁移记录并选择指定 relocation 执行回滚。</p>
    <label class="field">
      relocation 记录
      <select v-model="selectedRelocationId">
        <option v-for="item in listPayload" :key="item.relocation_id" :value="item.relocation_id">
          {{ item.app_id }} · {{ item.state }} · {{ item.relocation_id }}
        </option>
      </select>
    </label>
    <label class="field">
      <div style="display: flex; align-items: center; gap: 8px">
        <input id="force-rollback" v-model="forceRollback" type="checkbox" />
        <label for="force-rollback">强制回滚（建议开启）</label>
      </div>
    </label>
    <div style="display: flex; gap: 8px; margin-top: 8px; flex-wrap: wrap">
      <button type="button" :disabled="loading" @click="onList">刷新记录</button>
      <button type="button" :disabled="loading || !selectedRelocationId" @click="onRollback">
        {{ loading ? "执行中..." : "执行回滚" }}
      </button>
    </div>
    <pre v-if="selectedSummary">{{ JSON.stringify(selectedSummary, null, 2) }}</pre>
    <pre v-if="rollbackPayload">{{ JSON.stringify(rollbackPayload, null, 2) }}</pre>
    <pre v-if="error">{{ error }}</pre>
  </section>
</template>
