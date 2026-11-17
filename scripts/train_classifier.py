"""Train separate topic and geographic-scope classifiers with field-aware features."""
from pathlib import Path
import argparse,hashlib,json,struct,time
import numpy as np
from scipy.sparse import hstack
from sklearn.feature_extraction.text import TfidfVectorizer
from sklearn.svm import LinearSVC
from classifier_features import fields,field_features,normalize,weak_scope,fingerprint,flags,VIEWS,WEIGHTS
from classifier_examples import authored

TNEWS={'100':'society','101':'culture','102':'entertainment','103':'sports','104':'finance','106':'finance','107':'tech','108':'education','109':'tech','110':'military','112':'life','113':'world','114':'finance','115':'life','116':'tech'}
THUC={0:'sports',1:'entertainment',2:'finance',3:'education',4:'life',5:'china',6:'tech',7:'society',8:'tech',9:'finance'}

def label_topic(topic,title):
    f=flags(title)
    if f['legal_event'] and not f['fiction']:return 'society'
    if f['financial_event'] and not f['fiction']:return 'finance'
    if f['clinical'] and not f['fiction']:return 'health'
    if f['education'] and not f['financial_event']:return 'education'
    if f['government'] and not f['legal_event']:return 'china'
    if topic=='tech' and any(s in title.lower() for s in ['夺冠','赛事','战队','转会','赛季','比分','lpl','lck']):return 'sports'
    return topic

def encode_model(path,classifier,vectorizers):
    records=[];offset=0
    for role,vectorizer in zip(VIEWS,vectorizers):
        terms=vectorizer.get_feature_names_out()
        for i,term in enumerate(terms):records.append((fingerprint(role+'|'+term),float(vectorizer.idf_[i]),offset+i))
        offset+=len(terms)
    assert len({r[0] for r in records})==len(records),'Feature hash collision'
    scale=32760/max(float(np.abs(classifier.coef_).max()),1e-9)
    quantized=np.rint(classifier.coef_*scale).astype('<i2')
    with path.open('wb') as stream:
        stream.write(b'PNC2'+struct.pack('<IIf3f',len(records),len(classifier.classes_),1/scale,*WEIGHTS))
        for name,bias in zip(classifier.classes_,classifier.intercept_):
            value=name.encode('ascii');stream.write(struct.pack('<B',len(value))+value+struct.pack('<f',bias))
        for key,idf,index in sorted(records):stream.write(struct.pack('<Qf',key,idf)+quantized[:,index].tobytes())
    return {'classes':classifier.classes_.tolist(),'features':len(records),'bytes':path.stat().st_size,'sha256':hashlib.sha256(path.read_bytes()).hexdigest()}

def matrix(vectorizers,rows,fit=False):
    split=list(zip(*(fields(row) for row in rows)))
    matrices=[]
    for i,(vectorizer,texts) in enumerate(zip(vectorizers,split)):
        value=vectorizer.fit_transform(texts) if fit else vectorizer.transform(texts)
        matrices.append(value*WEIGHTS[i])
    return hstack(matrices,format='csr')

def main():
    parser=argparse.ArgumentParser();parser.add_argument('data',type=Path);parser.add_argument('--thuc',type=Path);parser.add_argument('--evaluation-pool',type=Path);args=parser.parse_args()
    root=Path(__file__).resolve().parents[1];work=args.data.parent/'classifier-v2'
    work.mkdir(exist_ok=True)
    def read(path):return [json.loads(s) for s in path.read_text(encoding='utf-8-sig').splitlines() if s.strip()]
    public_dev=read(args.data/'dev.json')
    protected={normalize(r['sentence']) for r in public_dev}
    for path in [work/'reported.jsonl',args.evaluation_pool]:
        if path and path.exists():protected.update(normalize(r['title']) for r in read(path))
    raw=[{'title':r['sentence'],'body':'','category':TNEWS[r['label']],'origin':'TNEWS'} for r in read(args.data/'train.json')]
    if args.thuc:raw.extend({'title':r['text'],'body':'','category':THUC[r['label']],'origin':'THUCNews'} for r in read(args.thuc))
    rows=[];seen=set()
    for row in raw:
        key=normalize(row['title'])
        if not key or key in seen or key in protected:continue
        if any(s in row['title'].lower().replace(' ','') for s in ['gta6','重建员工信任','全美航班','安世','闻泰']):continue
        seen.add(key);row['scope']=weak_scope(row['title'],row['category']);row['category']=label_topic(row['category'],row['title']);row['weight']=0.65;rows.append(row)
    examples=authored()
    for row in examples:
        if normalize(row['title']) not in protected:row['weight']=5.0;rows.append(row)
    # Teach lead/body views using real held-in titles, never held-out texts. The title view
    # remains present in most examples; a minority use a generic heading to require context.
    expanded=[]
    for i,row in enumerate(rows):
        expanded.append(row)
        if row['origin']!='authored-policy' and i%5==0:
            copy=dict(row);copy['body']=row['title'];copy['title']='最新消息' if i%10==0 else row['title'];copy['weight']=row['weight']*0.7;expanded.append(copy)
    rows=expanded
    print(json.dumps({'prepared_rows':len(rows),'public_titles':len(seen),'authored_contrasts':len(examples),'protected_titles':len(protected)}),flush=True)
    # Reproducible fitting, with no access to reported labels or evaluation labels.
    metadata={'format':'PNC2','algorithm':'Field-aware TF-IDF + balanced linear SVM; scope first, topic second','policy_sha256':hashlib.sha256((root/'training/classifier-policy.json').read_bytes()).hexdigest(),'training_rows':len(rows),'public_unique_titles':len(seen),'authored_contrasts':len(examples),'views':dict(zip(VIEWS,WEIGHTS)),'scope_margin':0.45,'topic_margin':0.45,'sources':[{'name':'TNEWS','sha256':hashlib.sha256((args.data/'train.json').read_bytes()).hexdigest()},{'name':'THUCNews','sha256':hashlib.sha256(args.thuc.read_bytes()).hexdigest() if args.thuc else None}],'protected_reported_rows':5}
    models={}
    for name,label,maximum in [('topic','category',(65000,22000,22000)),('scope','scope',(45000,16000,16000))]:
        selected=[row for row in rows if row.get(label) is not None and (name!='topic' or row[label]!='world')]
        vectorizers=[TfidfVectorizer(analyzer=field_features,min_df=2,max_features=n,sublinear_tf=True,dtype=np.float32) for n in maximum]
        print(f'[{name}] vectorizing {len(selected)} rows',flush=True);start=time.monotonic()
        x=matrix(vectorizers,selected,True)
        print(f'[{name}] fitting shape={x.shape}',flush=True)
        classifier=LinearSVC(C=0.7,class_weight='balanced',dual='auto',random_state=41,max_iter=3000)
        classifier.fit(x,[row[label] for row in selected],sample_weight=np.asarray([row['weight'] for row in selected]))
        target=root/'src'/('classifier-model.bin' if name=='topic' else 'classifier-scope.bin')
        info=encode_model(target,classifier,vectorizers);info['training_rows']=len(selected)
        metadata[name]=info;models[name]=(vectorizers,classifier)
        print(f'[{name}] done in {time.monotonic()-start:.1f}s; bytes={info["bytes"]}',flush=True)
    import joblib
    joblib.dump(models,work/'models-v2.joblib')
    # Export deterministic native/WASM parity controls, distinct from the user regressions.
    controls=examples[::max(1,len(examples)//16)][:16]
    golden=[]
    for row in controls:
        item={'title':row['title'],'body':row['body']}
        for name,(vec,clf) in models.items():
            scores=clf.decision_function(matrix(vec,[row]))[0];order=np.argsort(scores)[::-1]
            item[name]={'category':str(clf.classes_[order[0]]),'margin':float(scores[order[0]]-scores[order[1]])}
        golden.append(item)
    (root/'training/classifier-parity.json').write_text(json.dumps(golden,ensure_ascii=False,indent=2)+'\n',encoding='utf-8')
    (root/'src/classifier-model.json').write_text(json.dumps(metadata,ensure_ascii=False,indent=2)+'\n',encoding='utf-8')
    print('Both classifiers exported.',flush=True)

if __name__=='__main__':main()
