// Synthesized tabletop & naval audio cues via Web Audio API.
// Crisp, organic micro-sounds with zero asset weight.

let ctx: AudioContext | null = null;
let sfxMuted = false;

export function isSfxMuted(): boolean {
  return sfxMuted;
}

export function setSfxMuted(muted: boolean) {
  sfxMuted = muted;
}

function audioCtx(): AudioContext | null {
  if (sfxMuted || typeof window === "undefined") return null;
  const AC = window.AudioContext || (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
  if (!AC) return null;
  if (!ctx) {
    try {
      ctx = new AC();
    } catch {
      return null;
    }
  }
  if (ctx.state === "suspended") {
    void ctx.resume().catch(() => {});
  }
  return ctx;
}

function tone(freq: number, endFreq: number, dur: number, type: OscillatorType, gainPeak: number, delay = 0) {
  const c = audioCtx();
  if (!c) return;
  const t0 = c.currentTime + delay;
  const osc = c.createOscillator();
  const g = c.createGain();
  osc.type = type;
  osc.frequency.setValueAtTime(freq, t0);
  if (endFreq !== freq) {
    osc.frequency.exponentialRampToValueAtTime(Math.max(20, endFreq), t0 + dur);
  }
  g.gain.setValueAtTime(0.0001, t0);
  g.gain.linearRampToValueAtTime(gainPeak, t0 + Math.min(0.012, dur * 0.25));
  g.gain.exponentialRampToValueAtTime(0.0001, t0 + dur);
  osc.connect(g);
  g.connect(c.destination);
  osc.start(t0);
  osc.stop(t0 + dur + 0.02);
}

function noiseBurst(dur: number, filterFreq: number, q: number, gainPeak: number, delay = 0) {
  const c = audioCtx();
  if (!c) return;
  const t0 = c.currentTime + delay;
  const len = Math.max(1, Math.floor(c.sampleRate * dur));
  const buf = c.createBuffer(1, len, c.sampleRate);
  const data = buf.getChannelData(0);
  for (let i = 0; i < len; i++) {
    data[i] = (Math.random() * 2 - 1) * Math.exp(-i / (len * 0.38));
  }
  const src = c.createBufferSource();
  src.buffer = buf;
  const filt = c.createBiquadFilter();
  filt.type = "bandpass";
  filt.frequency.setValueAtTime(filterFreq, t0);
  filt.Q.setValueAtTime(q, t0);
  const g = c.createGain();
  g.gain.setValueAtTime(gainPeak, t0);
  g.gain.exponentialRampToValueAtTime(0.0001, t0 + dur);
  src.connect(filt);
  filt.connect(g);
  g.connect(c.destination);
  src.start(t0);
}

export const sfx = {
  move() {
    tone(240, 115, 0.055, "triangle", 0.14);
    noiseBurst(0.035, 950, 2.2, 0.08);
  },
  capture() {
    tone(190, 78, 0.085, "triangle", 0.2);
    noiseBurst(0.055, 720, 1.8, 0.14);
    tone(310, 150, 0.05, "sine", 0.1, 0.035);
  },
  check() {
    tone(587.33, 587.33, 0.14, "sine", 0.11);
    tone(880, 880, 0.22, "sine", 0.11, 0.06);
  },
  promote() {
    tone(440, 440, 0.12, "triangle", 0.1);
    tone(554.37, 554.37, 0.14, "triangle", 0.1, 0.07);
    tone(659.25, 659.25, 0.22, "triangle", 0.12, 0.14);
  },
  splash() {
    tone(320, 140, 0.14, "sine", 0.07);
    noiseBurst(0.18, 1300, 1.2, 0.09, 0.02);
  },
  hit() {
    tone(150, 48, 0.22, "sawtooth", 0.14);
    noiseBurst(0.22, 460, 0.9, 0.18);
  },
  sunk() {
    tone(130, 38, 0.36, "sawtooth", 0.18);
    noiseBurst(0.35, 340, 0.8, 0.22);
    tone(520, 390, 0.28, "sine", 0.08, 0.14);
  },
  win() {
    tone(523.25, 523.25, 0.14, "triangle", 0.11);
    tone(659.25, 659.25, 0.14, "triangle", 0.11, 0.09);
    tone(783.99, 783.99, 0.16, "triangle", 0.11, 0.18);
    tone(1046.5, 1046.5, 0.32, "sine", 0.13, 0.27);
  },
  lose() {
    tone(392, 370, 0.18, "triangle", 0.1);
    tone(349.23, 311.13, 0.28, "sine", 0.1, 0.14);
  },
};
