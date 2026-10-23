// Regression: a VAD-positive filler during a long answer must not erase speech.
// Real VAD/ASR/TTS and playback queue; WAV input + null output, debug builds only.
import {DatabaseSync} from 'node:sqlite';
import {spawn,execFileSync} from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import http from 'node:http';

const exe=path.resolve(process.argv[2]);
const clips=['name','story','filler'].map(n=>path.resolve('src-tauri/target/voice-qa',n+'.wav'));
for(const clip of clips) if(!fs.existsSync(clip)) throw new Error('Missing fixture '+clip);
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
const answer='Однажды инженер решил построить помощника, который внимательно слушает и всегда заканчивает свои ответы. '+
  'Он долго проверял каждую деталь, чтобы случайный шум за окном не мешал спокойному разговору. '+
  'Вечером помощник рассказал всю историю до конца, сохранив каждое предложение и последнее слово. Конец истории.';
const server=http.createServer((req,res)=>{
  req.resume();req.on('end',()=>{
    res.writeHead(200,{'content-type':'text/event-stream'});
    for(const part of answer.match(/.{1,12}/gsu))res.write('data: '+JSON.stringify({choices:[{index:0,delta:{content:part}}]})+'\n\n');
    res.end('data: '+JSON.stringify({choices:[{index:0,delta:{},finish_reason:'stop'}]})+'\n\ndata: [DONE]\n\n');
  });
});
await new Promise(r=>server.listen(18084,'127.0.0.1',r));
const dir=path.join(process.env.LOCALAPPDATA,'dev.jarvis.desktop');
const db=new DatabaseSync(path.join(dir,'jarvis.db'));
const backup=db.prepare("SELECT value FROM kv WHERE key='settings'").get().value;
const original=db.prepare('SELECT conversation_id AS id FROM messages ORDER BY id DESC LIMIT 1').get()?.id;
const cfg=JSON.parse(backup);
cfg.provider={preset:'ollama',baseUrl:'http://127.0.0.1:18084',model:'mock-model',visionModel:'',temperature:0.3};
cfg.voice={...cfg.voice,enabled:true,engine:'local',wakeWord:true};
cfg.privacy={...cfg.privacy,storeHistory:true};
const logFile=path.join(dir,'logs','jarvis.log');
const offset=fs.existsSync(logFile)?fs.statSync(logFile).size:0;
const since=new Date().toISOString();
let app;
try {
  db.prepare("UPDATE kv SET value=? WHERE key='settings'").run(JSON.stringify(cfg));
  app=spawn(exe,['--command','new-conversation'],{windowsHide:true,stdio:'ignore',env:{...process.env,RUST_LOG:'info',
    JARVIS_VOICE_INPUT_WAV:clips.join(';'),JARVIS_VOICE_OUTPUT:'null',JARVIS_VOICE_INPUT_GAP_S:'3'}});
  // Let the complete long answer drain, including its final sentence.
  await sleep(38000);
  const log=fs.readFileSync(logFile).subarray(offset).toString('utf8');
  const tasks=db.prepare('SELECT * FROM tasks WHERE started_at>=?').all(since);
  const phantomInterrupts=(log.match(/barge-in: speech cleared/g)??[]).length;
  const result={ok:tasks.length===1&&tasks[0].status==='completed'&&phantomInterrupts===0,
    phantomInterrupts,tasks:tasks.map(t=>({request:t.request,status:t.status,steps:t.steps})),
    ignoredNoise:log.includes('ignored non-speech during reply'),
    playbackDrained:log.includes('voice playback drained')};
  console.log(JSON.stringify(result));
  if(!result.ok)process.exitCode=1;
} finally {
  if(app){
    execFileSync(exe,['--command','cancel-task']);
    await sleep(200);
    const ids=db.prepare('SELECT DISTINCT conversation_id AS id FROM tasks WHERE started_at>=?').all(since);
    for(const {id} of ids) if(id!==original){
      db.prepare('DELETE FROM messages WHERE conversation_id=?').run(id);
      db.prepare('DELETE FROM tasks WHERE conversation_id=?').run(id);
      db.prepare('DELETE FROM conversations WHERE id=?').run(id);
    }
    execFileSync(exe,original?['--conversation',original]:['--command','new-conversation']);
    execFileSync(exe,['--command','quit']);
    for(let i=0;i<30&&app.exitCode===null;i++)await sleep(100);
    if(app.exitCode===null){app.kill();process.exitCode=1;}
  }
  db.prepare("UPDATE kv SET value=? WHERE key='settings'").run(backup);
  db.close();server.close();
}
