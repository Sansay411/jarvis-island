import { Slider } from "../../ui/Slider";
import { useSystem } from "../../store/system";

/** Volume and brightness, backed by Core Audio and WMI/DDC-CI. */
export function QuickControls() {
  const audio = useSystem((s) => s.audio);
  const brightness = useSystem((s) => s.brightness);
  const setVolume = useSystem((s) => s.setVolume);
  const toggleMute = useSystem((s) => s.toggleMute);
  const setBrightness = useSystem((s) => s.setBrightness);

  const supported = brightness?.filter((d) => d.supported) ?? [];
  const brightnessValue = supported[0]?.brightness ?? null;

  return (
    <div className="quick-controls">
      <Slider
        icon={audio?.muted ? "muted" : "volume"}
        label={audio ? `Громкость — ${audio.device}` : "Громкость"}
        value={audio ? (audio.muted ? 0 : audio.volume) : null}
        iconActive={audio?.muted}
        onIconClick={toggleMute}
        onChange={setVolume}
        disabled={!audio}
        disabledReason="Нет активного устройства вывода звука"
      />
      <Slider
        icon="sun"
        label="Яркость"
        value={brightnessValue}
        throttleMs={140}
        onChange={setBrightness}
        disabled={brightness !== null && supported.length === 0}
        disabledReason="Дисплей не поддерживает программную яркость"
      />
    </div>
  );
}
