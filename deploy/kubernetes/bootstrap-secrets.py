#!/usr/bin/env python3
"""Generate Foyer-only credentials privately. Requires explicit kubeconfig/context and prior approval.
Never prints secret values and never reads or reuses another application's credentials.
"""
import argparse, base64, json, os, secrets, subprocess
from pathlib import Path

parser=argparse.ArgumentParser()
parser.add_argument('--kubeconfig',required=True)
parser.add_argument('--context',required=True)
parser.add_argument('--private-dir',required=True)
args=parser.parse_args()
if 'ovh' in args.context.lower(): raise SystemExit('Refusing OVH context for this deployment.')
os.umask(0o077)
private=Path(args.private_dir).resolve()
if not str(private).startswith('/mnt/storage/'): raise SystemExit('Private staging must be on /mnt/storage.')
private.mkdir(parents=True,exist_ok=True,mode=0o700)
os.chmod(private,0o700)
kube=['kubectl','--kubeconfig='+args.kubeconfig,'--context='+args.context]
def run(argv,**kwargs): return subprocess.run(argv,check=True,capture_output=True,**kwargs)
# Confirm the selected cluster is reachable before generating any key material.
run(kube+['get','node','nobuntu','-o','name'])
run(kube+['apply','-f','-'],input=json.dumps({'apiVersion':'v1','kind':'Namespace','metadata':{'name':'foyer'}}).encode())
names=['foyer-database','foyer-dav','foyer-signing']
existing=[]
for name in names:
    result=subprocess.run(kube+['get','secret',name,'-n','foyer','--ignore-not-found','-o','name'],capture_output=True,check=True)
    existing.append(bool(result.stdout.strip()))
if all(existing):
    print('All dedicated Foyer runtime Secrets already exist; preserved unchanged.')
    raise SystemExit(0)
if any(existing): raise SystemExit('Partial runtime Secrets exist. Refusing to replace or mix credentials.')
path=private/'runtime-secrets.json'
if path.exists():
    bundle=json.loads(path.read_text())
else:
    passwords={key:secrets.token_urlsafe(36) for key in ['POSTGRES_PASSWORD','POWERSYNC_REPLICATION_PASSWORD','POWERSYNC_STORAGE_PASSWORD']}
    dav=secrets.token_urlsafe(36)
    key=run(['openssl','genpkey','-algorithm','EC','-pkeyopt','ec_paramgen_curve:P-256']).stdout
    users=run(['htpasswd','-niBC','12','foyer'],input=(dav+'\n').encode()).stdout
    def secret(name,values):
        return {'apiVersion':'v1','kind':'Secret','metadata':{'name':name,'namespace':'foyer'},'type':'Opaque',
                'data':{k:base64.b64encode(v if isinstance(v,bytes) else v.encode()).decode() for k,v in values.items()}}
    bundle={'apiVersion':'v1','kind':'List','items':[secret(names[0],passwords),secret(names[1],{'FOYER_DAV_PASSWORD':dav,'radicale-users':users}),secret(names[2],{'token-signing.pem':key})]}
    with path.open('x') as file: json.dump(bundle,file)
    os.chmod(path,0o600)
run(kube+['apply','-f','-'],input=json.dumps(bundle).encode())
print('Created the three dedicated Foyer runtime Secrets; no secret values displayed.')
