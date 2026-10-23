// Synthetic WAV -> actual VAD/ASR -> real native tool -> neural TTS.
// Debug executable only; no microphone, speaker output, cloud or credentials.
import {DatabaseSync} from 'node:sqlite';
import {spawn,execFileSync} from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
const exe=path.resolve(process.argv[2]);
const dir=path.join(process.env.LOCALAPPDATA,'dev.jarvis.desktop');
const db=new DatabaseSync(path.join(dir,'jarvis.db'));
const backup=db.prepare("SELECT value FROM kv WHERE key='settings'").get().value;
const original=db.prepare('SELECT conversation_id AS id FROM messages ORDER BY id DESC LIMIT 1').get()?.id;
const settings=JSON.parse(backup);
settings.provider={preset:'ollama',baseUrl:'http://127.0.0.1:18083',model:'mock-model',visionModel:'',temperature:0.3};
settings.voice={...settings.voice,enabled:true,engine:'local',wakeWord:true};
settings.privacy={...settings.privacy,storeHistory:true};
db.prepare("UPDATE kv SET value=? WHERE key='settings'").run(JSON.stringify(settings));
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
const fixture=name=>path.resolve('src-tauri/target/voice-qa',name+'.wav');
const files=['name','paused','stop','not-name','command'].map(fixture);
for(const file of files)if(!fs.existsSync(file))throw new Error('Missing fixture '+file);
const mockLog=path.resolve('src-tauri/target/voice-qa/mock.jsonl');
const server=spawn(process.execPath,['scripts/mock-llm.mjs','18083',mockLog],{stdio:'ignore',windowsHide:true});
const since=new Date().toISOString();
const logFile=path.join(dir,'logs','jarvis.log');
const logStart=fs.existsSync(logFile)?fs.statSync(logFile).size:0;
const app=spawn(exe,['--command','new-conversation'],{stdio:'ignore',windowsHide:true,env:{...process.env,RUST_LOG:'info',JARVIS_VOICE_INPUT_WAV:files.join(';'),JARVIS_VOICE_OUTPUT:'null',JARVIS_VOICE_INPUT_GAP_S:'3'}});
let tasks=[];
let report;
try {
 for(let i=0;i<240;i++){
  await sleep(250);
  tasks=db.prepare('SELECT * FROM tasks WHERE started_at>=? ORDER BY started_at').all(since);
  if(tasks.length>=2&&tasks.some(t=>t.request.toLowerCase().includes('джарвис'))&&tasks.every(t=>t.status==='completed')){await sleep(1500);break;}
 }
 const logs=fs.readFileSync(logFile).subarray(logStart).toString('utf8');
 const tools=db.prepare("SELECT payload,tool_ok FROM messages WHERE created_at>=? AND tool_ok IS NOT NULL").all(since);
 const early=[...logs.matchAll(/voice: early name detected ms=(\d+)/g)].map(m=>Number(m[1]));
 const firstAudio=[...logs.matchAll(/voice time to first audio ms=(\d+)/g)].map(m=>Number(m[1]));
 const ok=tasks.length===2&&tasks.every(t=>t.status==='completed')&&tools.length===2&&tools.every(t=>t.tool_ok===1)&&early.length>=2&&logs.includes('stop word')&&!logs.includes('speech recognition failed');
 report={ok,tasks:tasks.map(t=>({request:t.request,status:t.status,ms:Date.parse(t.finished_at)-Date.parse(t.started_at),steps:t.steps})),earlyWakeMs:early,firstAudioMs:firstAudio,nativeTools:tools.length,stopRecognized:logs.includes('stop word')};
 console.log(JSON.stringify(report));
 if(!ok)process.exitCode=1;
}finally{
 execFileSync(exe,['--command','cancel-task']);
 await sleep(200);
 for(const id of new Set(tasks.map(t=>t.conversation_id))){
  db.prepare('DELETE FROM messages WHERE conversation_id=?').run(id);
  db.prepare('DELETE FROM tasks WHERE conversation_id=?').run(id);
  db.prepare('DELETE FROM conversations WHERE id=?').run(id);
 }
 execFileSync(exe,original?['--conversation',original]:['--command','new-conversation']);
 execFileSync(exe,['--command','quit']);
 for(let i=0;i<30&&app.exitCode===null;i++)await sleep(100);
 if(app.exitCode===null){app.kill();console.error('Debug app needed forced test cleanup');process.exitCode=1;}
 db.prepare("UPDATE kv SET value=? WHERE key='settings'").run(backup);
 server.kill();db.close();
}
