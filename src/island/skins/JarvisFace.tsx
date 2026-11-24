// Single entry point rendering JARVIS's face in whichever skin is active
// (either passed explicitly via `skin` or read from `usePrefs`). The running
// tool's activity is held a moment so quick calls don't flicker.

import { usePrefs } from "../../store/prefs";
import { AmberSkin } from "./AmberSkin";
import { IceSkin } from "./IceSkin";
import { MinimalSkin } from "./MinimalSkin";
import { OrbSkin } from "./OrbSkin";
import { PixelSkin } from "./PixelSkin";
import { normalizeSkin, type EyesProps } from "./types";
import { useSteadyActivity } from "./useSteadyActivity";

export function JarvisFace(input: EyesProps) {
  const prefSkin = usePrefs((s) => s.skin);
  const skin = input.skin ? normalizeSkin(input.skin) : prefSkin;
  const activity = useSteadyActivity(input.activity);
  const props: EyesProps = { ...input, activity };
  switch (skin) {
    case "orb":
      return <OrbSkin {...props} />;
    case "ice":
      return <IceSkin {...props} />;
    case "pixel":
      return <PixelSkin {...props} />;
    case "minimal":
      return <MinimalSkin {...props} />;
    case "amber":
    default:
      return <AmberSkin {...props} />;
  }
}
