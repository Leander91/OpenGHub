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
    const locking = !locked;
    try {
      configStore.apply(await api.setProfileLock(deviceId, feature, locking));
      ui.toast(
        locking ? "Locked: the same in every profile, from this one." : "Now set per profile.",
        "success",
        2500,
      );
    } catch (e) {
      ui.toast(api.errorMessage(e), "error");
    }
  }
</script>

<button class="lock" class:on={locked} onclick={toggle} title={locked ? "Click to set per profile again" : "Click to use these settings in every profile"}>
  <Icon name="lock" size={16} strokeWidth={locked ? 2.2 : 1.6} />
  <span class="text">
    <strong>{locked ? "Locked: same in every profile" : "Per profile"}</strong>
    <small>{locked ? "Click to unlock" : "Click the lock to use these settings in every profile"}</small>
  </span>
</button>

<style>
  .lock {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 8px 10px;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-sm);
    text-align: left;
    color: var(--text-dim);
  }

  .lock:hover {
    background: var(--surface-2);
  }

  .lock.on {
    border-color: var(--accent, #1196ff);
    color: var(--accent, #1196ff);
  }

  .text {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  strong {
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }

  small {
    font-size: 11px;
    color: var(--text-dimmer);
  }
</style>
