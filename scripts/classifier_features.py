"""Shared training features for the two-stage news classifier."""
from pathlib import Path
import json,re
from functools import lru_cache
POLICY=json.loads((Path(__file__).resolve().parents[1]/'training/classifier-policy.json').read_text(encoding='utf-8-sig'))
VIEWS=('t','l','b')
WEIGHTS=(1.0,0.45,0.15)
def clean(text):
    text=text.strip()
    text=''.join(chr(ord(c)-0xfee0) if '！'<=c<='～' else c for c in text).lower()
    lines=[]
    for line in text.splitlines():
        line=line.strip()
        if any(line.startswith(s.lower()) for s in POLICY['sources']):
            first,sep,rest=line.partition(',')
            if sep and len(first)<=55 and (first.endswith('电') or '消息' in first):line=rest
        if line:lines.append(line)
    return '\n'.join(lines)
def units(text):
    result=[];word='';number=False
    for c in clean(text):
        if c.isascii() and c.isalnum():
            if c.isdigit():
                if not number:word+='#'
                number=True
            else:word+=c;number=False
        else:
            if word:result.append('@'+word);word=''
            number=False
            if c.isalnum():result.append(c)
    if word:result.append('@'+word)
    return result
def normalize(text):return ''.join(units(text))
def entity_position(text,term):
    term=term.lower()
    if term.isascii():
        match=re.search(r'(?<![a-z0-9])'+re.escape(term)+r'(?![a-z0-9])',text)
        return match.start() if match else -1
    if term=='国产':
        for match in re.finditer(term,text):
            if not any(text[max(0,match.start()+1-len(country)):match.start()+1]==country.lower() for country in POLICY['foreign_places'] if country.endswith('国')):return match.start()
        return -1
    return text.find(term)
@lru_cache(maxsize=160000)
def flags(text):
    text=clean(text)
    result={}
    for key in ('foreign_places','foreign_actors','domestic_actors','domestic_places','domestic_context','foreign_reports','guide_markers','news_actions'):
        positions=[position for s in POLICY[key] if (position:=entity_position(text,s))>=0]
        result[key]=bool(positions)
        result[key+'_position']=min(positions,default=10**9)
    result['cn_equity']=bool(re.search(r'(?:\*st|a股|沪指|沪深|深交所|上交所|科创板|创业板)',text))
    foreign=min(result['foreign_places_position'],result['foreign_actors_position'])
    domestic=result['domestic_actors_position']
    if result['cn_equity']:
        equity=re.search(r'(?:\*st|a股|沪指|沪深|深交所|上交所|科创板|创业板)',text)
        domestic=min(domestic,equity.start())
    result['first_foreign']=foreign<domestic and foreign<10**9
    result['first_domestic']=domestic<foreign and domestic<10**9
    result['mixed']=result['foreign_places'] and result['domestic_actors']
    result['local_foreign_brand']=result['foreign_actors'] and result['domestic_context']
    result['foreign_report_domestic']=result['foreign_reports'] and result['domestic_actors']
    result['legal_event']=any(s in text for s in ['法院','法庭','裁定','判决','上诉','起诉','诉讼','仲裁','审理','刑拘','立案'])
    result['financial_event']=any(s in text for s in ['财报','营收','净利润','股价','股指','收益率','利率','汇率','融资','债券','期货'])
    result['fiction']=any(s in text for s in ['游戏中','游戏里','游戏玩法','虚构角色','虚构城市','剧情','小说'])
    result['fiction_law']=result['fiction'] and result['legal_event']
    result['fiction_finance']=result['fiction'] and result['financial_event']
    result['clinical']=any(s.lower() in text for s in POLICY['clinical_terms'])
    for topic,words in POLICY['topic_hints'].items():result['hint_'+topic]=any(entity_position(text,s)>=0 for s in words)
    result['education']=any(s in text for s in ['招生','高考','中考','录取','学费','教学','课程','助学','入学'])
    result['military']=any(s in text for s in ['军演','实弹','部队','战机','导弹','航母','军队','国防'])
    result['government']=any(s in text for s in ['国务院','全国人大','全国政协','党委','中央纪委','党籍','省委','政务'])
    result['diplomatic_event']=any(s in text for s in ['外长','外交部','外交会谈','中方代表','联合国','国事访问']) and any(s in text for s in ['会谈','通话','访问','谈判','峰会','双边关系'])
    return {key:value for key,value in result.items() if not key.endswith('_position')}
def field_features(text):
    token=units(text)
    output=[]
    for size in range(1,5):
        output.extend('g:'+'\x1f'.join(token[i:i+size]) for i in range(len(token)-size+1))
    output.extend('k:'+key for key,value in flags(text).items() if value)
    return output
def fields(record):
    title=clean(record.get('title',''))[:240]
    body=clean(record.get('body',''))
    return title,body[:320],body[:1600]
def weak_scope(title,topic):
    f=flags(title)
    if f['cn_equity'] or f['domestic_context']:return 'domestic'
    if f['diplomatic_event']:return 'foreign'
    if f['guide_markers'] and not f['news_actions']:return 'unspecified'
    if f['foreign_report_domestic']:return None
    if f['mixed']:return None
    if f['domestic_actors']:return 'domestic'
    if f['foreign_places']:return 'foreign'
    if f['foreign_actors'] and not f['guide_markers']:return 'foreign'
    if f['foreign_reports']:return 'foreign'
    if topic=='world':return 'foreign'
    if f['domestic_places']:return 'domestic'
    return 'unspecified'
def fingerprint(text):
    value=0xcbf29ce484222325
    for byte in text.encode('utf-8'):value=((value^byte)*0x100000001b3)&0xffffffffffffffff
    return value
