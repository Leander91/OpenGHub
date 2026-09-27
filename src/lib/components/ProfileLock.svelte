<script lang="ts">
  /**
   * G HUB's per-profile lock: keep one feature of this device the same in
   * every profile. Locking copies the active profile's settings everywhere.
   */
  import * as api from "$lib/api";
  import Icon from "$lib/components/Icon.svelte";
  import { configStore } from "$lib/stores/config.svelte";
  import { ui } from "$lib/stores/ui.svelte";

  interface Props {
    deviceId: string;
    /** `lighting` | `assignments` | `dpi` | `gamemode` */
    feature: string;
  }
  let { deviceId, feature }: Props = $props();

  const locked = $derived((configStore.settings.locks?.[deviceId] ?? []).includes(feature));

  async function toggle() {
    try {
      configStore.apply(await api.setProfileLock(deviceId, feature, !locked));
      ui.toast(
        locked ? "Now set per profile." : "Locked: the same in every profile, from this one.",
        "success",
        2500,
      );
    } catch (e) {
      ui.toast(api.errorMessage(e), "error");
    }
  }
</script>

<button class="lock" class:on={locked} onclick={toggle} title={locked ? "Same in every profile — click to set per profile" : "Set per profile — click to use the same in every profile"}>
  <Icon name="lock" size={14} />
  <span>{locked ? "Persistent configuration" : "Per-profile configuration"}</span>
</button>

<style>
  .lock {
    display: flex;
    align-items: center;
    gap: 8px;
    align-self: flex-start;
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-dimmer);
  }

  .lock.on {
    color: var(--primary, #1196ff);
  }
</style>
