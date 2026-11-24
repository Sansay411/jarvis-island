// JARVIS's eyes — the product's identity. Stable wrapper delegating to the
// active skin component (`src/island/skins/*`) so every surface that renders
// `<Eyes />` automatically follows the chosen `appearance.skin` setting.

import { JarvisFace } from "./skins/JarvisFace";
import type { EyeMood, EyesProps, SkinId } from "./skins/types";

export type { EyeMood, EyesProps, SkinId };
export { JarvisFace };

export function Eyes(props: EyesProps) {
  return <JarvisFace {...props} />;
}
