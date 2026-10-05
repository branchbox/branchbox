#!/usr/bin/env node
/** Deterministic local canvas render. PLAYWRIGHT_MODULE and CHROMIUM_PATH may override discovery. */
import { createRequire } from 'node:module';
import { spawn } from 'node:child_process';
import {verifyAssets} from './verify-assets.mjs';
import { once } from 'node:events';
import { writeFile, mkdir, readFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const require=createRequire(import.meta.url);
const {chromium}=require(process.env.PLAYWRIGHT_MODULE||'playwright');
const dir=resolve(dirname(fileURLToPath(import.meta.url)),'..'),args=process.argv.slice(2);
const variant=args[0]||'launch',mode=args[1]||'proof',output=resolve(args[2]||'/tmp/branchbox-videos');
if(!['launch','setup','teardown'].includes(variant)||!['proof','movie','beats','purity'].includes(mode))throw new Error('Usage: render.mjs launch|setup|teardown proof|movie|beats|purity output-directory');
await verifyAssets(dir,variant);
await mkdir(output,{recursive:true});
const browser=await chromium.launch({headless:true,...(process.env.CHROMIUM_PATH?{executablePath:process.env.CHROMIUM_PATH}:{})});
const page=await browser.newPage({viewport:{width:1440,height:1440},deviceScaleFactor:1});
const server=createServer(async(req,res)=>{try{const path=resolve(dir,'assets','.'+decodeURIComponent(req.url.split('?')[0]));if(!path.startsWith(resolve(dir,'assets')+'/'))throw new Error('path outside assets');const data=await readFile(path);res.end(data);}catch{res.statusCode=404;res.end();}});
await new Promise(r=>server.listen(0,'127.0.0.1',r));
await page.goto(`http://127.0.0.1:${server.address().port}/film.html?variant=${variant}`);
await page.evaluate(variant=>seek(0,variant),variant);
const duration=variant==='launch'?27:14;
const frame=async t=>Buffer.from(await page.evaluate(({t,variant})=>encodeFrame(t,variant),{t,variant}),'base64');
try{
 if(mode==='movie'){
  const movie=resolve(output,`branchbox-${variant}.mp4`);
  const ff=spawn(process.env.FFMPEG||'ffmpeg',['-hide_banner','-loglevel','warning','-y','-f','image2pipe','-framerate','240','-vcodec','mjpeg','-i','pipe:0','-vf',"tmix=frames=4:weights='1 1 1 1',select='not(mod(n+1,4))',setpts=N/(60*TB)",'-r','60','-c:v','libx264','-preset','fast','-crf','18','-pix_fmt','yuv420p','-movflags','+faststart','-an',movie],{stdio:['pipe','inherit','inherit']});
  let failure=null;ff.on('error',e=>failure=e);
  for(let n=0;n<duration*240;n++){
   const data=await frame(Math.min(duration,(n+.5)/240));
   if(!ff.stdin.write(data))await once(ff.stdin,'drain');
   if(n%240===0)console.log(`${variant}: ${n/240}/${duration}s`);
   if(failure)throw failure;
  }
  ff.stdin.end();const [code]=await once(ff,'close');if(code!==0)throw new Error(`ffmpeg exit ${code}`);
  await writeFile(resolve(output,`${variant}-render.json`),JSON.stringify({variant,duration,width:1440,height:1440,fps:60,subframes:4,inputFps:240,blend:'ffmpeg tmix 4 equal weights; select every fourth blended frame',audio:'none',movie},null,2)+'\n');
 }else if(mode==='purity'){
  const {createHash}=await import('node:crypto');let samples=[0,.5,3,6,10,13,17,19.5,23,26.8,27].filter(t=>t<=duration),forward={};
  for(const t of samples){const f=await frame(t);forward[t]=createHash('sha256').update(f).digest('hex');await writeFile(resolve(output,`${variant}-purity-first-${t}.jpg`),f);}
  for(const t of samples.reverse()){const f=await frame(t);if(forward[t]!==createHash('sha256').update(f).digest('hex')){await writeFile(resolve(output,`${variant}-purity-second-${t}.jpg`),f);throw new Error(`Non-deterministic seek ${t}`);}}
  await writeFile(resolve(output,`${variant}-purity.json`),JSON.stringify({pass:true,samples:forward},null,2)+'\n');console.log('Time purity passed');
 }else{
  const samples=mode==='proof'?(variant==='launch'?[['open',.5],['glass',10],['stage',17],['workspace',23]]:[['poster',variant==='setup'?8:6]]):Array.from({length:duration*2},(_,i)=>[`beat-${String(i+1).padStart(2,'0')}`,i/2+.08]);
  for(const [name,t]of samples){await page.evaluate(({t,variant})=>seek(t,variant),{t,variant});await page.screenshot({path:resolve(output,`${variant}-${name}.png`)});}
 }
}finally{await browser.close();await new Promise(r=>server.close(r))}
