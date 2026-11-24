// Single entry point rendering JARVIS's face in whichever skin is active
// (either passed explicitly via `skin` or read from `usePrefs`).

import { usePrefs } from "../../store/prefs";
import { AmberSkin } from "./AmberSkin";
import { IceSkin } from "./IceSkin";
import { MinimalSkin } from "./MinimalSkin";
import { OrbSkin } from "./OrbSkin";
import { PixelSkin } from "./PixelSkin";
import { normalizeSkin, type EyesProps } from "./types";

export function JarvisFace(props: EyesProps) {
  const prefSkin = usePrefs((s) => s.skin);
  const skin = props.skin ? normalizeSkin(props.skin) : prefSkin;
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
