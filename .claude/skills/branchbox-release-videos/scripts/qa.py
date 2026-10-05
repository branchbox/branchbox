#!/usr/bin/env python3
"""Decode each film, inspect frame-difference spikes, and make readable beat sheets.
Requires Pillow and NumPy. No source edits or publishing side effects.
"""
import argparse, hashlib, json, subprocess
from pathlib import Path
import numpy as np
from PIL import Image, ImageDraw
p=argparse.ArgumentParser();p.add_argument('movie',type=Path);p.add_argument('--expected-duration',type=float,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args();a.output.mkdir(parents=True,exist_ok=True)
probe=json.loads(subprocess.check_output(['ffprobe','-v','error','-show_streams','-show_format','-of','json',str(a.movie)]));video=next(s for s in probe['streams'] if s['codec_type']=='video');duration=float(probe['format']['duration']);assert abs(duration-a.expected_duration)<.02,(duration,a.expected_duration);assert (video['width'],video['height'])==(1440,1440);assert video['r_frame_rate']=='60/1';assert not any(s['codec_type']=='audio' for s in probe['streams']);
subprocess.run(['ffmpeg','-v','error','-i',str(a.movie),'-f','null','-'],check=True)
raw=subprocess.check_output(['ffmpeg','-v','error','-i',str(a.movie),'-vf','scale=240:240','-pix_fmt','gray','-f','rawvideo','-']);frames=np.frombuffer(raw,dtype=np.uint8).reshape(-1,240,240);diff=np.abs(np.diff(frames.astype(np.float32),axis=0)).mean(axis=(1,2));spikes=[{'frame':i+1,'time':(i+1)/60,'difference':float(diff[i]),'previous':float(diff[i-1]),'next':float(diff[i+1])} for i in range(1,len(diff)-1) if diff[i]>1 and diff[i]>3*max(diff[i-1],diff[i+1],.02)]
loop=float(np.abs(frames[0].astype(np.float32)-frames[-1]).mean());sheet=Image.new('RGB',(6*244, int(np.ceil(duration*2/6))*264),'#f6f4ee');d=ImageDraw.Draw(sheet)
for n in range(int(duration*2)):
 out=a.output/f'beat-{n+1:02}.png';subprocess.run(['ffmpeg','-v','error','-y','-ss',str(n/2+.08),'-i',str(a.movie),'-frames:v','1','-vf','scale=240:240',str(out)],check=True);im=Image.open(out);x=(n%6)*244;y=(n//6)*264;sheet.paste(im,(x,y));d.text((x+7,y+243),f'Beat {n+1} · {n/2:.1f}s',fill='#171817')
sheet.save(a.output/'beat-contact.png')
record={'movie':str(a.movie),'sha256':hashlib.sha256(a.movie.read_bytes()).hexdigest(),'duration':duration,'dimensions':[video['width'],video['height']],'fps':60,'frames':len(frames),'audio':'none','full_decode':'passed','spike_rule':'mean grayscale frame difference >1 and >3× both neighbors','flagged_spikes':spikes,'loop_mean_grayscale_difference':loop,'visual_review':'pending; inspect beat contact and each flagged spike before integration'}
(a.output/'qa.json').write_text(json.dumps(record,indent=2)+'\n');print(json.dumps({k:record[k] for k in ['duration','frames','flagged_spikes','loop_mean_grayscale_difference']},indent=2))
