// Measures actual WebView2 frames through Jarvis's existing instrumentation.
import {execFileSync} from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
const exe=process.argv[2];
const log=path.join(process.env.LOCALAPPDATA,'dev.jarvis.desktop','logs','jarvis.log');
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
const results=[];
for(let i=0;i<4;i++) {
    const start=fs.statSync(log).size;
    execFileSync(exe,['--command','open-workspace']);
    let sample;
    for(let j=0;j<40;j++) {
      await sleep(100);
      const tail=fs.readFileSync(log).subarray(start).toString('utf8');
      const m=tail.match(/island animation label="workspace [^"]+" fps=(\d+) worst_ms=(\d+) frames=(\d+)/);
      if(m) { sample={open:i+1,fps:Number(m[1]),worstMs:Number(m[2]),frames:Number(m[3])};break; }
    }
    if(!sample) throw new Error('No workspace frame sample; voice view may be open or logging disabled');
    results.push(sample);
    execFileSync(exe,['--command','toggle-workspace']);
    await sleep(800);
}
console.log(JSON.stringify(results));
