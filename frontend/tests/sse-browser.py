"""Two-tab integration test against an isolated real backend.
Requires built backend/frontend, Firefox, geckodriver, and Python selenium.
Run: python3 tests/sse-browser.py
"""
import http.cookiejar
import json
import os
from pathlib import Path
import secrets
import shutil
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
work = Path(tempfile.mkdtemp(prefix='hwaiting-sse-e2e-'))
with socket.socket() as listener:
    listener.bind(('127.0.0.1', 0))
    port = listener.getsockname()[1]
base = f'http://localhost:{port}'
key = secrets.token_hex(32)
env = {name: value for name, value in os.environ.items() if not name.startswith('HWAITING_')}
for name in ('CREDENTIALS_DIRECTORY', 'HWAITING_UNIX_SOCKET', 'LISTEN_PID', 'LISTEN_FDS'):
    env.pop(name, None)
env.update({
    'HWAITING_DATABASE_URL': f'sqlite://{work}/test.db',
    'HWAITING_JWT_SECRET': key,
    'HWAITING_ADMIN_PASSWORD': 'cookie-test-password',
    'HWAITING_ADMIN_USERNAME': 'cookie-test-admin',
    'HWAITING_HOST': '127.0.0.1', 'HWAITING_PORT': str(port),
    'HWAITING_COOKIE_PARTITIONED': 'false',
    'HWAITING_COOKIE_SECURE': 'false', 'HWAITING_COOKIE_SAME_SITE': 'lax',
    'HWAITING_JWT_EXPIRY_SECONDS': '3600',
    'HWAITING_RP_ID': 'localhost', 'HWAITING_RP_ORIGINS': base,
    'HWAITING_CORS_ALLOWED_ORIGINS': 'http://localhost:5173',
    'HWAITING_STATIC_DIR': str(ROOT / 'frontend/dist'),
    'RUST_LOG': 'error',
})
server = None
driver = None
log = (work / 'server.log').open('ab')

def start_server():
    global server
    server = subprocess.Popen([str(ROOT / 'backend/target/debug/hwaiting')], env=env, cwd=work, stdout=log, stderr=log)
    for _ in range(200):
        if server.poll() is not None:
            raise AssertionError('Backend exited: ' + (work / 'server.log').read_text())
        try:
            with urllib.request.urlopen(base + '/api/health', timeout=0.2) as response:
                if response.status == 200:
                    return
        except (urllib.error.URLError, TimeoutError):
            time.sleep(0.05)
    raise AssertionError('Backend startup timed out')

def stop_server():
    if server and server.poll() is None:
        server.terminate()
        server.wait(timeout=10)

jar = http.cookiejar.CookieJar()
client = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(jar))
plain = urllib.request.build_opener()

def request(path, method='GET', body=None, csrf=True, headers=None, opener=client):
    headers = dict(headers or {})
    data = None
    if body is not None:
        data = json.dumps(body).encode()
        headers['Content-Type'] = 'application/json'
    if method not in ('GET', 'HEAD', 'OPTIONS') and csrf:
        headers['X-Requested-With'] = 'hwaiting'
    req = urllib.request.Request(base + path, data=data, headers=headers, method=method)
    try:
        response = opener.open(req, timeout=10)
    except urllib.error.HTTPError as error:
        response = error
    raw = response.read()
    try:
        parsed = json.loads(raw)
    except (json.JSONDecodeError, UnicodeDecodeError):
        parsed = raw
    return response.status, response.headers, parsed

import sqlite3
from selenium import webdriver
from selenium.common.exceptions import StaleElementReferenceException
from selenium.webdriver.common.by import By
from selenium.webdriver.support.ui import WebDriverWait
from selenium.webdriver.firefox.options import Options
from selenium.webdriver.common.action_chains import ActionChains
try:
    static = work / 'static'
    shutil.copytree(ROOT / 'frontend/dist', static)
    env['HWAITING_STATIC_DIR'] = str(static)
    start_server()
    assert request('/api/auth/signup', 'POST', {'username':'sse-user','password':'sse-test-password'})[0] == 201
    assert request('/api/user/settings','PATCH',{'auto_progress_on_correct':False})[0] == 200
    options=Options();options.add_argument('-headless')
    driver=webdriver.Firefox(options=options)
    wait=WebDriverWait(driver,15,ignored_exceptions=(StaleElementReferenceException,))
    driver.get(base)
    driver.add_cookie({'name':'hwaiting-auth','value':next(c.value for c in jar if c.name=='hwaiting-auth'),'path':'/'})
    # Observe browser requests and streams without changing production code.
    instrument="""
    window.sseStreams=[]; window.reads=[]; window.settingsWrites=[]; window.nextReplies=[]; window.blockNext=false;
    const RealSource=window.EventSource;
    window.EventSource=class extends RealSource { constructor(...args) {super(...args);window.sseStreams.push(this)} };
    const realFetch=window.fetch;
    window.fetch=async (...args)=>{window.reads.push(String(args[0]));
    if(String(args[0]).endsWith('/user/settings') && args[1]?.method==='PATCH') window.settingsWrites.push(JSON.parse(args[1].body));
    const result=await realFetch(...args);
    if(String(args[0]).includes('/cards/next')) {if(window.blockNext) await new Promise(r=>window.releaseNext=r);window.nextReplies.push(await result.clone().json())}
    if(String(args[0]).endsWith('/check')) {if(window.holdCheck) await new Promise(r=>window.releaseCheck=r);else await new Promise(r=>setTimeout(r,700));}return result};
    """
    # Inject before application scripts by serving a transient instrumented index.
    index=Path(env['HWAITING_STATIC_DIR'])/'index.html'
    original=index.read_text()
    index.write_text(original.replace('<head>','<head><script>'+instrument+'</script>'))
    driver.refresh()
    first=driver.current_window_handle
    wait.until(lambda d: d.find_elements(By.CSS_SELECTOR,'article[data-card-id]'))
    cid=int(driver.find_element(By.CSS_SELECTOR,'article[data-card-id]').get_attribute('data-card-id'))
    driver.switch_to.new_window('tab'); second=driver.current_window_handle;driver.get(base)
    wait.until(lambda d: d.find_elements(By.CSS_SELECTOR,'article[data-card-id]'))
    assert int(driver.find_element(By.CSS_SELECTOR,'article[data-card-id]').get_attribute('data-card-id'))==cid
    driver.find_element(By.CSS_SELECTOR,'input[aria-label="Answer"]').send_keys('preserve my answer')
    wait.until(lambda d: d.execute_script('return window.sseStreams.some(s=>s.readyState===1)'))
    driver.execute_script('window.reads=[]')
    # Card far down the queue: replenish the new-card successor, preserving active work.
    other=40 if cid!=40 else 41
    revision=request(f'/api/cards/{other}/state')[2]['review_revision']
    assert request(f'/api/cards/{other}/check','POST',{'answer':'wrong','review_revision':revision})[0]==200
    wait.until(lambda d: any('/cards/stats' in p for p in d.execute_script('return window.reads')))
    wait.until(lambda d: any('/cards/next' in p for p in d.execute_script('return window.reads')))
    assert int(driver.find_element(By.CSS_SELECTOR,'article[data-card-id]').get_attribute('data-card-id'))==cid
    assert driver.find_element(By.CSS_SELECTOR,'input[aria-label="Answer"]').get_attribute('value')=='preserve my answer'
    driver.switch_to.window(first)
    with sqlite3.connect(work/'test.db') as db:
        answer=db.execute('SELECT tg.form FROM sentences s JOIN targets tg ON tg.sentence_id=s.id WHERE s.card_id=?',(cid,)).fetchone()[0]
    driver.find_element(By.CSS_SELECTOR,'input[aria-label="Answer"]').send_keys(answer)
    driver.find_element(By.CSS_SELECTOR,'button[type="submit"]').click()
    wait.until(lambda d:'Correct! The answer was' in d.find_element(By.TAG_NAME,'body').text)
    assert 'already been reviewed' not in driver.find_element(By.TAG_NAME,'body').text
    driver.switch_to.window(second)
    wait.until(lambda d: d.find_elements(By.CSS_SELECTOR,'article[data-card-id]') and int(d.find_element(By.CSS_SELECTOR,'article[data-card-id]').get_attribute('data-card-id'))!=cid)
    assert 'already been reviewed' not in driver.find_element(By.TAG_NAME,'body').text
    assert driver.find_element(By.CSS_SELECTOR,'input[aria-label="Answer"]').get_attribute('value')==''
    # Close the stream deliberately to miss a review, then reconnect.
    newcid=int(driver.find_element(By.CSS_SELECTOR,'article[data-card-id]').get_attribute('data-card-id'))
    driver.find_element(By.CSS_SELECTOR,'input[aria-label="Answer"]').send_keys('keep through reconnect')
    driver.execute_script('window.sseStreams.forEach(s=>s.close())')
    revision=request(f'/api/cards/{newcid}/state')[2]['review_revision']
    assert request(f'/api/cards/{newcid}/check','POST',{'answer':'wrong','review_revision':revision})[0]==200
    driver.execute_script("window.dispatchEvent(new Event('online'))")
    wait.until(lambda d: d.find_elements(By.CSS_SELECTOR,'article[data-card-id]') and int(d.find_element(By.CSS_SELECTOR,'article[data-card-id]').get_attribute('data-card-id'))!=newcid)
    assert 'changed while you were away' not in driver.find_element(By.TAG_NAME,'body').text
    assert driver.find_element(By.CSS_SELECTOR,'input[aria-label="Answer"]').get_attribute('value')==''
    # A genuinely empty queue wakes up on a remote settings change.
    assert request('/api/auth/signup','POST',{'username':'empty-user','password':'sse-test-password'})[0]==201
    assert request('/api/user/settings','PATCH',{'daily_new_card_limit':0})[0]==200
    driver.add_cookie({'name':'hwaiting-auth','value':next(c.value for c in jar if c.name=='hwaiting-auth'),'path':'/'})
    driver.refresh()
    wait.until(lambda d: not d.find_elements(By.CSS_SELECTOR,'article[data-card-id]') and 'No cards' in d.find_element(By.TAG_NAME,'body').text)
    wait.until(lambda d: d.execute_script('return window.sseStreams.some(s=>s.readyState===1)'))
    assert request('/api/user/settings','PATCH',{'daily_new_card_limit':1})[0]==200
    wait.until(lambda d:d.find_elements(By.CSS_SELECTOR,'article[data-card-id]'))
    cid=int(driver.find_element(By.CSS_SELECTOR,'article[data-card-id]').get_attribute('data-card-id'))
    driver.find_element(By.CSS_SELECTOR,'input[aria-label="Answer"]').send_keys('keep on suppression')
    assert request(f'/api/cards/{cid}/suppress','PUT')[0]==200
    wait.until(lambda d:'suppressed elsewhere' in d.find_element(By.TAG_NAME,'body').text)
    assert driver.find_element(By.CSS_SELECTOR,'input[aria-label="Answer"]').get_attribute('value')=='keep on suppression'
    # An edit made in this tab is already applied; reconnect must not mark it stale.
    assert request('/api/auth/login','POST',{'username':env['HWAITING_ADMIN_USERNAME'],'password':env['HWAITING_ADMIN_PASSWORD']})[0]==200
    assert request('/api/user/settings','PATCH',{'auto_progress_on_correct':False})[0]==200
    driver.add_cookie({'name':'hwaiting-auth','value':next(c.value for c in jar if c.name=='hwaiting-auth'),'path':'/'})
    driver.refresh()
    wait.until(lambda d:d.find_elements(By.CSS_SELECTOR,'article[data-card-id]'))
    driver.find_element(By.CSS_SELECTOR,'input[aria-label="Answer"]').send_keys('wrong')
    driver.find_element(By.CSS_SELECTOR,'button[type="submit"]').click()
    wait.until(lambda d:'Incorrect. You typed' in d.find_element(By.TAG_NAME,'body').text)
    driver.find_element(By.CSS_SELECTOR,'button[aria-label="Card options"]').click()
    wait.until(lambda d:d.find_element(By.XPATH,"//*[@role='menuitem' and contains(., 'Edit card')]" )).click()
    label=wait.until(lambda d:d.find_element(By.XPATH,"//label[text()='Translation word (English)']"))
    field=driver.find_element(By.ID,label.get_attribute('for'))
    field.clear();field.send_keys('edited in this tab')
    driver.find_element(By.XPATH,"//button[text()='Save']").click()
    wait.until(lambda d:not d.find_elements(By.CSS_SELECTOR,'[role="dialog"]'))
    driver.execute_script("window.reads=[];window.sseStreams.forEach(s=>s.close());window.dispatchEvent(new Event('online'))")
    wait.until(lambda d:any('/state' in p for p in d.execute_script('return window.reads')))
    time.sleep(0.5)
    assert 'changed while you were away' not in driver.find_element(By.TAG_NAME,'body').text
    assert 'Incorrect. You typed' in driver.find_element(By.TAG_NAME,'body').text
    # With due review cards, a ready successor can be promoted even while all
    # subsequent next-card responses are blocked. Only the new prefetch waits.
    assert request('/api/auth/signup','POST',{'username':'due-user','password':'sse-test-password'})[0]==201
    assert request('/api/user/settings','PATCH',{'daily_new_card_limit':0,'auto_progress_on_correct':False})[0]==200
    with sqlite3.connect(work/'test.db') as db:
        uid=db.execute("SELECT id FROM users WHERE username='due-user'").fetchone()[0]
        for card_id in (1,2,3,4,5):
            db.execute("INSERT INTO cards_states(user_id,card_id,stability,difficulty,last_review,state,review_revision,due_at) VALUES(?,?,1,3,?,'review',0,0)",(uid,card_id,f'2020-01-0{card_id}T00:00:00Z'))
            db.execute("INSERT INTO review_history(user_id,card_id,rating,reviewed_at,stability,difficulty,state) VALUES(?,?,'good',?,1,3,'review')",(uid,card_id,f'2020-01-0{card_id}T00:00:00Z'))
    driver.add_cookie({'name':'hwaiting-auth','value':next(c.value for c in jar if c.name=='hwaiting-auth'),'path':'/'})
    driver.refresh()
    wait.until(lambda d:d.find_elements(By.CSS_SELECTOR,'article[data-card-id]') and d.find_element(By.CSS_SELECTOR,'article[data-card-id]').get_attribute('data-card-id')=='1')
    wait.until(lambda d:d.execute_script('return window.nextReplies.some(r=>r.card?.card_id===2)'))
    time.sleep(0.2)
    driver.execute_script('window.reads=[]')
    assert request('/api/cards/5/check','POST',{'answer':'wrong','review_revision':0})[0]==200
    wait.until(lambda d:any('/cards/stats' in p for p in d.execute_script('return window.reads')))
    assert not any('/cards/next' in p for p in driver.execute_script('return window.reads'))
    driver.execute_script('window.reads=[];window.blockNext=true')
    assert request('/api/cards/1/check','POST',{'answer':'wrong','review_revision':0})[0]==200
    wait.until(lambda d:d.find_elements(By.CSS_SELECTOR,'article[data-card-id]') and d.find_element(By.CSS_SELECTOR,'article[data-card-id]').get_attribute('data-card-id')=='2')
    wait.until(lambda d:d.execute_script('return !!window.releaseNext'))
    assert len([p for p in driver.execute_script('return window.reads') if '/cards/next' in p])==1
    driver.execute_script('window.blockNext=false;window.releaseNext();window.releaseNext=null')
    wait.until(lambda d:d.execute_script('return window.nextReplies.some(r=>r.card?.card_id===3)'))
    # Invalidate that successor, hold its replacement, then press Next. The
    # existing replacement request must be consumed, not duplicated.
    driver.find_element(By.CSS_SELECTOR,'input[aria-label="Answer"]').send_keys('wrong')
    driver.find_element(By.CSS_SELECTOR,'button[type="submit"]').click()
    wait.until(lambda d:'Incorrect. You typed' in d.find_element(By.TAG_NAME,'body').text)
    driver.execute_script('window.reads=[];window.blockNext=true')
    assert request('/api/cards/3/suppress','PUT')[0]==200
    wait.until(lambda d:d.execute_script('return !!window.releaseNext'))
    driver.find_element(By.XPATH,"//button[text()='Next']").click()
    time.sleep(0.2)
    assert len([p for p in driver.execute_script('return window.reads') if '/cards/next' in p])==1
    driver.execute_script('window.blockNext=false;window.releaseNext();window.releaseNext=null')
    wait.until(lambda d:d.find_elements(By.CSS_SELECTOR,'article[data-card-id]') and d.find_element(By.CSS_SELECTOR,'article[data-card-id]').get_attribute('data-card-id')=='4')
    # A remote review can supersede an attempt whose successful HTTP response
    # is delayed. The late result must not reintroduce the discarded card/reveal.
    driver.execute_script('window.holdCheck=true')
    driver.find_element(By.CSS_SELECTOR,'input[aria-label="Answer"]').send_keys('wrong')
    driver.find_element(By.CSS_SELECTOR,'button[type="submit"]').click()
    wait.until(lambda d:d.execute_script('return !!window.releaseCheck'))
    assert request('/api/cards/4/state')[2]['review_revision']==1
    assert request('/api/cards/4/check','POST',{'answer':'wrong','review_revision':1})[0]==200
    wait.until(lambda d:not d.find_elements(By.CSS_SELECTOR,'article[data-card-id]') and 'No cards' in d.find_element(By.TAG_NAME,'body').text)
    driver.execute_script('window.releaseCheck()')
    time.sleep(0.5)
    assert 'Incorrect. You typed' not in driver.find_element(By.TAG_NAME,'body').text
    # A retention drag previews locally, then atomically reschedules once on release.
    driver.find_element(By.CSS_SELECTOR,'button[aria-label="Account menu"]').click()
    wait.until(lambda d:d.find_element(By.XPATH,"//*[@role='menuitem' and contains(., 'Settings')]" )).click()
    wait.until(lambda d:d.find_element(By.XPATH,"//button[contains(., 'Review settings')]" )).click()
    thumb=wait.until(lambda d:d.find_element(By.CSS_SELECTOR,'[aria-labelledby="desired-retention-label"]')).find_element(By.XPATH,'ancestor::*[@data-slot="slider-thumb"]')
    time.sleep(0.6)  # Let the accordion finish expanding before locating the drag target.
    driver.execute_script("arguments[0].scrollIntoView({block:'center'})",thumb)
    before=request('/api/user/export')[2]
    with sqlite3.connect(work/'test.db') as db:
        before_due=db.execute('SELECT due_at FROM cards_states WHERE user_id=? AND card_id=1',(uid,)).fetchone()[0]
    driver.execute_script('window.settingsWrites=[]')
    ActionChains(driver).move_to_element(thumb).click_and_hold().move_by_offset(-40,0).pause(0.1).move_by_offset(-40,0).perform()
    assert driver.execute_script('return window.settingsWrites.length')==0
    ActionChains(driver).release().perform()
    wait.until(lambda d:d.execute_script('return window.settingsWrites.length')==1)
    changed=driver.execute_script('return window.settingsWrites[0].desired_retention')
    assert changed < before['settings']['desired_retention']
    wait.until(lambda d:request('/api/user/settings')[2]['desired_retention']==changed)
    after=request('/api/user/export')[2]
    assert before['review_history']==after['review_history']
    with sqlite3.connect(work/'test.db') as db:
        after_due=db.execute('SELECT due_at FROM cards_states WHERE user_id=? AND card_id=1',(uid,)).fetchone()[0]
    assert after_due > before_due
    print('PASS: background refill, automatic remote advancement, warm successor promotion, in-flight refill reuse, late response isolation, own echoes, reconnect, empty queue, suppression, own edits, retention drag and rescheduling')
finally:
    if 'original' in globals(): index.write_text(original)
    if driver: driver.quit()
    stop_server();log.close()
    shutil.rmtree(work)
