import { createModels, fauxProvider } from '@earendil-works/pi-ai';
import { openAgent } from '../src/runtime.mjs';
const faux=fauxProvider(); const models=createModels(); models.setProvider(faux.provider);
let reached; const started=new Promise(resolve=>{reached=resolve;});
faux.setResponses([(_ctx,options)=>new Promise((_,reject)=>{reached(); options.signal.addEventListener('abort',()=>reject(options.signal.reason),{once:true});})]);
const agent=await openAgent({directory:process.argv[2],models,model:{provider:'faux',modelId:'faux-1'}});
const result=await agent.submit({requestId:'crash-input',content:'Survive restart'}); await started; process.send({id:result.submissionId});
