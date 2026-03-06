import { beforeEach, describe, expect, it } from "vitest";
import { nextTick } from "vue";
import { useI18n } from "../../src/i18n";

describe("i18n", () => {
  beforeEach(() => {
    window.localStorage.clear();
  });

  it("supports locale switching and interpolation", async () => {
    const { t, setLocale, locale } = useI18n();

    setLocale("zh");
    expect(locale.value).toBe("zh");
    expect(t("app.systemDisk.free", { free: "45 GB" })).toBe("剩余 45 GB");

    setLocale("en");
    expect(locale.value).toBe("en");
    expect(t("app.systemDisk.free", { free: "45 GB" })).toBe("Free 45 GB");
    await nextTick();
    expect(window.localStorage.getItem("disk-relocator.locale")).toBe("en");
  });

  it("returns key path when translation key does not exist", () => {
    const { t } = useI18n();
    expect(t("not.exists.translation.key")).toBe("not.exists.translation.key");
  });

  it("keeps placeholder token when interpolation param is missing", () => {
    const { t, setLocale } = useI18n();
    setLocale("en");
    expect(t("app.messages.restoreDone")).toContain("{name}");
  });

  it("supports numeric interpolation in both locales", () => {
    const { t, setLocale } = useI18n();

    setLocale("zh");
    expect(t("health.issueCount", { count: 3 })).toBe("检测到 3 项异常");

    setLocale("en");
    expect(t("health.issueCount", { count: 3 })).toBe("3 issue(s) detected");
  });
});
