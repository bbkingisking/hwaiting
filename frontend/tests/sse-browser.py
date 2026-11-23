"""Independent browser integration scenarios against isolated seeded backends.
Requires built backend/frontend, Firefox, geckodriver, and Python selenium.
Run all: python3 tests/sse-browser.py
Run one: python3 tests/sse-browser.py BrowserScenarios.test_empty_queue_wakes_and_suppression_discards_the_prompt
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

import sqlite3
import unittest

from selenium import webdriver
from selenium.common.exceptions import StaleElementReferenceException
from selenium.webdriver.common.by import By
from selenium.webdriver.support.ui import WebDriverWait
from selenium.webdriver.firefox.options import Options
from selenium.webdriver.common.action_chains import ActionChains
from selenium.webdriver.common.keys import Keys

ROOT = Path(__file__).resolve().parents[2]

# Installed only in the temporary static copy, before application scripts.
# Hold actual responses to exercise races without changing production code.
INSTRUMENT = """
window.sseStreams = [];
window.reads = [];
window.settingsWrites = [];
window.nextReplies = [];
const RealSource = window.EventSource;
window.EventSource = class extends RealSource {
    constructor(...args) {
        super(...args);
        window.sseStreams.push(this);
    }
};
const realFetch = window.fetch;
window.fetch = async (...args) => {
    const url = String(args[0]);
    window.reads.push(url);
    if (url.endsWith('/user/settings') && args[1]?.method === 'PATCH') {
        window.settingsWrites.push(JSON.parse(args[1].body));
        if (window.failSettingsOnce) {
            window.failSettingsOnce = false;
            throw new Error('simulated settings failure');
        }
        if (window.timeoutSettingsOnce) {
            window.timeoutSettingsOnce = false;
            return await new Promise((_, reject) =>
                args[1].signal.addEventListener('abort', () =>
                    reject(new DOMException('aborted', 'AbortError'))));
        }
    }
    const result = await realFetch(...args);
    if (url.includes('/reviews/queue')) {
        if (window.blockNext) await new Promise(resolve => window.releaseNext = resolve);
        window.nextReplies.push(...(await result.clone().json()).cards);
    }
    if (url.endsWith('/reviews/answer')) {
        if (window.holdCheck) await new Promise(resolve => window.releaseCheck = resolve);
        else await new Promise(resolve => setTimeout(resolve, 700));
        window.answerReturned = true;
    }
    return result;
};
"""


class BrowserScenarios(unittest.TestCase):
    """Each scenario gets a fresh seeded backend; browser sessions share no app state."""

    def start_server(self):
        self.server = subprocess.Popen([str(ROOT / 'backend/target/debug/hwaiting')], env=self.env, cwd=self.work, stdout=self.log, stderr=self.log)
        for _ in range(200):
            if self.server.poll() is not None:
                raise AssertionError('Backend exited: ' + (self.work / 'server.log').read_text())
            try:
                with urllib.request.urlopen(self.base + '/api/health', timeout=0.2) as response:
                    if response.status == 200:
                        return
            except (urllib.error.URLError, TimeoutError):
                time.sleep(0.05)
        raise AssertionError('Backend startup timed out')

    def stop_server(self):
        if self.server and self.server.poll() is None:
            self.server.terminate()
            self.server.wait(timeout=10)

    def request(self, path, method='GET', body=None, csrf=True, headers=None, opener=None):
        headers = dict(headers or {})
        data = None
        if body is not None:
            data = json.dumps(body).encode()
            headers['Content-Type'] = 'application/json'
        if method not in ('GET', 'HEAD', 'OPTIONS') and csrf:
            headers['X-Requested-With'] = 'hwaiting'
        req = urllib.request.Request(self.base + path, data=data, headers=headers, method=method)
        try:
            response = (opener or self.client).open(req, timeout=10)
        except urllib.error.HTTPError as error:
            response = error
        raw = response.read()
        try:
            parsed = json.loads(raw)
        except (json.JSONDecodeError, UnicodeDecodeError):
            parsed = raw
        return response.status, response.headers, parsed

    def answer_card(self, card_id):
        status, _, queue = self.request('/api/reviews/queue')
        assert status == 200
        entry = next(entry for entry in queue['cards'] if entry['card']['card_id'] == card_id)
        return self.request('/api/reviews/answer', 'POST', {
            'answer': 'wrong', 'attempt_token': entry['review']['attempt_token'],
        })

    @classmethod
    def setUpClass(cls):
        options = Options()
        options.add_argument('-headless')
        cls.browser = webdriver.Firefox(options=options)
        cls.addClassCleanup(cls.browser.quit)

    def setUp(self):
        self.work = Path(tempfile.mkdtemp(prefix='hwaiting-browser-'))
        self.addCleanup(shutil.rmtree, self.work)
        with socket.socket() as listener:
            listener.bind(('127.0.0.1', 0))
            port = listener.getsockname()[1]
        self.base = f'http://localhost:{port}'
        self.env = {name: value for name, value in os.environ.items() if not name.startswith('HWAITING_')}
        for name in ('CREDENTIALS_DIRECTORY', 'LISTEN_PID', 'LISTEN_FDS'):
            self.env.pop(name, None)
        self.env.update({
            'HWAITING_DATABASE_URL': f'sqlite://{self.work}/test.db',
            'HWAITING_JWT_SECRET': secrets.token_hex(32),
            'HWAITING_SIGNUPS_ENABLED': 'true',
            'HWAITING_ADMIN_USERNAME': 'browser-admin',
            'HWAITING_ADMIN_PASSWORD': 'browser-test-password',
            'HWAITING_HOST': '127.0.0.1', 'HWAITING_PORT': str(port),
            'HWAITING_COOKIE_PARTITIONED': 'false',
            'HWAITING_COOKIE_SECURE': 'false', 'HWAITING_COOKIE_SAME_SITE': 'lax',
            'HWAITING_RP_ID': 'localhost', 'HWAITING_RP_ORIGINS': self.base,
            'HWAITING_STATIC_DIR': str(self.work / 'static'), 'RUST_LOG': 'error',
        })
        shutil.copytree(ROOT / 'frontend/dist', self.work / 'static')
        index = self.work / 'static/index.html'
        index.write_text(index.read_text().replace('<head>', '<head><script>' + INSTRUMENT + '</script>'))
        self.log = (self.work / 'server.log').open('ab')
        self.addCleanup(self.log.close)
        self.server = None
        self.addCleanup(self.stop_server)
        self.start_server()
        self.jar = http.cookiejar.CookieJar()
        self.client = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(self.jar))
        self.driver = self.browser
        for handle in self.driver.window_handles[1:]:
            self.driver.switch_to.window(handle)
            self.driver.close()
        self.driver.switch_to.window(self.driver.window_handles[0])
        self.driver.get(self.base)
        self.driver.delete_all_cookies()
        self.wait = WebDriverWait(self.driver, 15, ignored_exceptions=(StaleElementReferenceException,))
        assert self.request('/api/auth/signup', 'POST', {'username': 'scenario-user', 'password': 'browser-test-password'})[0] == 201
        # The seeded content always includes English. Complete L1 onboarding explicitly.
        assert self.request('/api/user/settings', 'PATCH', {'translation_language': 'eng', 'auto_progress_on_correct': False})[0] == 200
        self.open_account()
        self.wait.until(lambda d: d.find_elements(By.CSS_SELECTOR, 'button[aria-label="Account menu"]'))

    def open_account(self):
        self.driver.add_cookie({'name': 'hwaiting-auth', 'value': next(c.value for c in self.jar if c.name == 'hwaiting-auth'), 'path': '/'})
        self.driver.refresh()

    def admin_account(self):
        assert self.request('/api/auth/login', 'POST', {'username': self.env['HWAITING_ADMIN_USERNAME'], 'password': self.env['HWAITING_ADMIN_PASSWORD']})[0] == 200
        assert self.request('/api/user/settings', 'PATCH', {'translation_language': 'eng', 'auto_progress_on_correct': False})[0] == 200
        self.open_account()
        self.wait.until(lambda d: d.find_elements(By.CSS_SELECTOR, 'article[data-card-id]'))

    def due_account(self):
        assert self.request('/api/user/settings', 'PATCH', {'daily_new_card_limit': 0})[0] == 200
        with sqlite3.connect(self.work / 'test.db') as db:
            uid = db.execute("SELECT id FROM users WHERE username='scenario-user'").fetchone()[0]
            for card_id in (1, 2, 3, 4, 5):
                reviewed_at = f'2020-01-0{card_id}T00:00:00Z'
                db.execute("INSERT INTO cards_states(user_id,card_id,stability,difficulty,last_review,state,review_revision,due_at) VALUES(?,?,1,3,?,'review',0,0)", (uid, card_id, reviewed_at))
                db.execute("INSERT INTO review_history(user_id,card_id,rating,reviewed_at,stability,difficulty,state) VALUES(?,?,'good',?,1,3,'review')", (uid, card_id, reviewed_at))
        self.driver.refresh()
        self.wait.until(lambda d: d.find_elements(By.CSS_SELECTOR, 'article[data-card-id]') and d.find_element(By.CSS_SELECTOR, 'article[data-card-id]').get_attribute('data-card-id') == '1')
        self.wait.until(lambda d: d.execute_script('return window.nextReplies.some(r=>r.card?.card_id===2)'))
        return uid

    def settings(self, section):
        self.driver.find_element(By.CSS_SELECTOR, 'button[aria-label="Account menu"]').click()
        self.wait.until(lambda d: d.find_element(By.XPATH, "//*[@role='menuitem' and contains(., 'Settings')]" )).click()
        self.wait.until(lambda d: d.find_element(By.XPATH, f"//button[contains(., '{section}')]" )).click()
        # Wait for the panel to finish opening before scrolling or clicking its controls.
        self.driver.execute_async_script("""
            const done = arguments[arguments.length - 1];
            requestAnimationFrame(() => requestAnimationFrame(() => {
                const animations = Array.from(document.querySelectorAll('[role="dialog"]'))
                    .flatMap(dialog => dialog.getAnimations({subtree: true}));
                Promise.all(animations.map(animation => animation.finished.catch(() => {})))
                    .then(() => done());
            }));
        """)

    def test_remote_reviews_preserve_unrelated_work_and_resync_on_reconnect(self):
        first=self.driver.current_window_handle
        self.wait.until(lambda d: d.find_elements(By.CSS_SELECTOR,'article[data-card-id]'))
        cid=int(self.driver.find_element(By.CSS_SELECTOR,'article[data-card-id]').get_attribute('data-card-id'))
        self.driver.switch_to.new_window('tab'); second=self.driver.current_window_handle;self.driver.get(self.base)
        self.wait.until(lambda d: d.find_elements(By.CSS_SELECTOR,'article[data-card-id]'))
        assert int(self.driver.find_element(By.CSS_SELECTOR,'article[data-card-id]').get_attribute('data-card-id'))==cid
        self.driver.find_element(By.CSS_SELECTOR,'input[aria-label="Answer"]').send_keys('preserve my answer')
        self.wait.until(lambda d: d.execute_script('return window.sseStreams.some(s=>s.readyState===1)'))
        self.driver.execute_script('window.reads=[]')
        # Card far down the queue: replenish the new-card successor, preserving active work.
        other=self.request('/api/reviews/queue')[2]['cards'][-1]['card']['card_id']
        assert other != cid
        assert self.answer_card(other)[0]==200
        self.wait.until(lambda d: any('/reviews/stats' in p for p in d.execute_script('return window.reads')))
        self.wait.until(lambda d: any('/reviews/queue' in p for p in d.execute_script('return window.reads')))
        assert int(self.driver.find_element(By.CSS_SELECTOR,'article[data-card-id]').get_attribute('data-card-id'))==cid
        assert self.driver.find_element(By.CSS_SELECTOR,'input[aria-label="Answer"]').get_attribute('value')=='preserve my answer'
        self.driver.switch_to.window(first)
        with sqlite3.connect(self.work/'test.db') as db:
            answer=db.execute('SELECT tg.form FROM sentences s JOIN targets tg ON tg.sentence_id=s.id WHERE s.card_id=?',(cid,)).fetchone()[0]
        self.driver.find_element(By.CSS_SELECTOR,'input[aria-label="Answer"]').send_keys(answer)
        self.driver.find_element(By.CSS_SELECTOR,'button[type="submit"]').click()
        self.wait.until(lambda d:'Correct! The answer was' in d.find_element(By.TAG_NAME,'body').text)
        self.driver.switch_to.window(second)
        self.wait.until(lambda d: d.find_elements(By.CSS_SELECTOR,'article[data-card-id]') and int(d.find_element(By.CSS_SELECTOR,'article[data-card-id]').get_attribute('data-card-id'))!=cid)
        assert self.driver.find_element(By.CSS_SELECTOR,'input[aria-label="Answer"]').get_attribute('value')==''
        # Close the stream deliberately to miss a review, then reconnect.
        newcid=int(self.driver.find_element(By.CSS_SELECTOR,'article[data-card-id]').get_attribute('data-card-id'))
        self.driver.find_element(By.CSS_SELECTOR,'input[aria-label="Answer"]').send_keys('keep through reconnect')
        self.driver.execute_script('window.sseStreams.forEach(s=>s.close())')
        assert self.answer_card(newcid)[0]==200
        self.driver.execute_script("window.dispatchEvent(new Event('online'))")
        self.wait.until(lambda d: d.find_elements(By.CSS_SELECTOR,'article[data-card-id]') and int(d.find_element(By.CSS_SELECTOR,'article[data-card-id]').get_attribute('data-card-id'))!=newcid)
        assert self.driver.find_element(By.CSS_SELECTOR,'input[aria-label="Answer"]').get_attribute('value')==''

    def test_empty_queue_wakes_and_suppression_discards_the_prompt(self):
        assert self.request('/api/user/settings','PATCH',{'translation_language':'eng','daily_new_card_limit':0})[0]==200
        self.driver.add_cookie({'name':'hwaiting-auth','value':next(c.value for c in self.jar if c.name=='hwaiting-auth'),'path':'/'})
        self.driver.refresh()
        self.wait.until(lambda d: not d.find_elements(By.CSS_SELECTOR,'article[data-card-id]') and 'No cards' in d.find_element(By.TAG_NAME,'body').text)
        self.wait.until(lambda d: d.execute_script('return window.sseStreams.some(s=>s.readyState===1)'))
        assert self.request('/api/user/settings','PATCH',{'daily_new_card_limit':1})[0]==200
        self.wait.until(lambda d:d.find_elements(By.CSS_SELECTOR,'article[data-card-id]'))
        cid=int(self.driver.find_element(By.CSS_SELECTOR,'article[data-card-id]').get_attribute('data-card-id'))
        self.driver.find_element(By.CSS_SELECTOR,'input[aria-label="Answer"]').send_keys('keep on suppression')
        assert self.request(f'/api/cards/{cid}/suppress','PUT')[0]==200
        self.wait.until(lambda d: not d.find_elements(By.CSS_SELECTOR, f'article[data-card-id="{cid}"]'))
        self.wait.until(lambda d: d.find_elements(By.CSS_SELECTOR, 'article[data-card-id]'))
        assert self.driver.find_element(By.CSS_SELECTOR, 'input[aria-label="Answer"]').get_attribute('value') == ''

    def edit_hanja(self):
        label = self.wait.until(lambda d: d.find_element(By.XPATH, "//*[@role='dialog']//label[text()='Hanja']"))
        field = self.driver.find_element(By.ID, label.get_attribute('for'))
        field.clear()
        field.send_keys('學')
        self.driver.find_element(By.XPATH, "//*[@role='dialog']//button[text()='Save changes']").click()

    def test_local_edit_preserves_an_accepted_review_until_next(self):
        self.admin_account()
        card_id = self.driver.find_element(By.CSS_SELECTOR, 'article[data-card-id]').get_attribute('data-card-id')
        self.driver.find_element(By.CSS_SELECTOR, 'input[aria-label="Answer"]').send_keys('wrong')
        self.driver.find_element(By.CSS_SELECTOR, 'button[type="submit"]').click()
        self.wait.until(lambda d: 'Incorrect. You typed' in d.find_element(By.TAG_NAME, 'body').text)
        self.driver.find_element(By.CSS_SELECTOR, 'button[aria-label="Card options"]').click()
        self.wait.until(lambda d: d.find_element(By.XPATH, "//*[@role='menuitem' and contains(., 'Edit card')]" )).click()
        self.edit_hanja()
        self.wait.until(lambda d: not d.find_elements(By.CSS_SELECTOR, '[role="dialog"]'))
        self.driver.execute_script("window.reads=[];window.sseStreams.forEach(s=>s.close());window.dispatchEvent(new Event('online'))")
        self.wait.until(lambda d: d.execute_script('return window.sseStreams.some(s=>s.readyState===1)'))
        assert self.driver.find_element(By.CSS_SELECTOR, 'article[data-card-id]').get_attribute('data-card-id') == card_id
        assert 'Incorrect. You typed' in self.driver.find_element(By.TAG_NAME, 'body').text
        assert self.request(f'/api/admin/cards/{card_id}')[2]['hanja'] == '學'
        self.driver.find_element(By.XPATH, "//button[text()='Next']").click()
        self.wait.until(lambda d: d.find_elements(By.CSS_SELECTOR, 'article[data-card-id]') and d.find_element(By.CSS_SELECTOR, 'article[data-card-id]').get_attribute('data-card-id') != card_id)

    def test_browse_edit_refreshes_the_unanswered_prompt(self):
        self.admin_account()
        card_id = self.driver.find_element(By.CSS_SELECTOR, 'article[data-card-id]').get_attribute('data-card-id')
        self.driver.find_element(By.CSS_SELECTOR, 'input[aria-label="Answer"]').send_keys('old draft')
        self.driver.find_element(By.CSS_SELECTOR, 'button[aria-label="Account menu"]').click()
        self.wait.until(lambda d: d.find_element(By.XPATH, "//*[@role='menuitem' and contains(., 'Browse Cards')]" )).click()
        self.driver.find_element(By.CSS_SELECTOR, 'input[aria-label="Search cards by target or card id"]').send_keys(card_id)
        self.wait.until(lambda d: d.find_element(By.CSS_SELECTOR, f'[role="dialog"] button[data-card-id="{card_id}"]')).click()
        self.edit_hanja()
        self.wait.until(lambda d: not d.find_elements(By.XPATH, "//*[@role='dialog' and .//h2[starts-with(text(), 'Edit card #')]]"))
        ActionChains(self.driver).send_keys(Keys.ESCAPE).perform()
        self.wait.until(lambda d: not d.find_elements(By.CSS_SELECTOR, '[role="dialog"]'))
        self.wait.until(lambda d: d.execute_script(
            'return window.nextReplies.some(entry => entry.card.card_id === Number(arguments[0]) && entry.card.content_revision === 1)', card_id))
        self.wait.until(lambda d: d.find_elements(By.CSS_SELECTOR, 'input[aria-label="Answer"]') and d.find_element(By.CSS_SELECTOR, 'input[aria-label="Answer"]').get_attribute('value') == '')
        assert self.driver.find_element(By.CSS_SELECTOR, 'input[aria-label="Answer"]').get_attribute('value') == ''
        assert self.request(f'/api/admin/cards/{card_id}')[2]['hanja'] == '學'

    def test_missed_content_edit_is_rejected_without_recording_a_review(self):
        assert self.request('/api/user/settings','PATCH',{'translation_language':'eng','daily_new_card_limit':0})[0]==200
        with sqlite3.connect(self.work/'test.db') as db:
            stale_uid=db.execute("SELECT id FROM users WHERE username='scenario-user'").fetchone()[0]
            db.execute("INSERT INTO cards_states(user_id,card_id,stability,difficulty,last_review,state,review_revision,due_at) VALUES(?,40,1,3,'2020-01-01T00:00:00Z','review',0,0)",(stale_uid,))
            db.execute("INSERT INTO review_history(user_id,card_id,rating,reviewed_at,stability,difficulty,state) VALUES(?,40,'good','2020-01-01T00:00:00Z',1,3,'review')",(stale_uid,))
        self.driver.add_cookie({'name':'hwaiting-auth','value':next(c.value for c in self.jar if c.name=='hwaiting-auth'),'path':'/'})
        self.driver.refresh()
        self.wait.until(lambda d:d.find_elements(By.CSS_SELECTOR,'article[data-card-id]') and d.find_element(By.CSS_SELECTOR,'article[data-card-id]').get_attribute('data-card-id')=='40')
        self.driver.execute_script('window.sseStreams.forEach(s=>s.close())')
        assert self.request('/api/auth/login','POST',{'username':self.env['HWAITING_ADMIN_USERNAME'],'password':self.env['HWAITING_ADMIN_PASSWORD']})[0]==200
        detail = self.request('/api/admin/cards/40')[2]
        assert self.request('/api/admin/cards/40', 'PATCH', {'expected_content_revision': detail['content_revision'], 'changes': [{'translation_language': 'eng', 'sentence_id': 40, 'sentence': '새 답입니다.', 'target': '답'}]})[0] == 200
        self.driver.find_element(By.CSS_SELECTOR,'input[aria-label="Answer"]').send_keys('old answer')
        self.driver.find_element(By.CSS_SELECTOR,'button[type="submit"]').click()
        self.wait.until(lambda d:d.execute_script('return window.nextReplies.some(r=>r.card?.card_id===40 && r.card?.content_revision===1)'))
        self.wait.until(lambda d:d.find_elements(By.CSS_SELECTOR,'article[data-card-id]') and d.find_element(By.CSS_SELECTOR,'article[data-card-id]').get_attribute('data-card-id')=='40' and d.find_element(By.CSS_SELECTOR,'input[aria-label="Answer"]').get_attribute('value')=='')
        assert 'Incorrect. You typed' not in self.driver.find_element(By.TAG_NAME,'body').text
        with sqlite3.connect(self.work/'test.db') as db:
            assert db.execute('SELECT COUNT(*) FROM review_history WHERE user_id=? AND card_id=40',(stale_uid,)).fetchone()[0]==1

    def test_buffered_successor_promotes_while_refill_is_pending(self):
        self.due_account()
        self.driver.execute_script('window.blockNext=true')
        self.driver.find_element(By.CSS_SELECTOR, 'input[aria-label="Answer"]').send_keys('wrong')
        self.driver.find_element(By.CSS_SELECTOR, 'button[type="submit"]').click()
        self.wait.until(lambda d: 'Incorrect. You typed' in d.find_element(By.TAG_NAME, 'body').text)
        self.wait.until(lambda d: d.execute_script('return !!window.releaseNext'))
        self.driver.find_element(By.XPATH, "//button[text()='Next']").click()
        self.wait.until(lambda d: d.find_elements(By.CSS_SELECTOR, 'article[data-card-id]') and d.find_element(By.CSS_SELECTOR, 'article[data-card-id]').get_attribute('data-card-id') == '2')
        self.driver.execute_script('window.blockNext=false;window.releaseNext();window.releaseNext=null')
        self.wait.until(lambda d: d.execute_script('return window.nextReplies.some(r=>r.card?.card_id===3)'))

    def test_suppressed_successor_is_not_promoted_during_pending_refill(self):
        self.due_account()
        self.driver.find_element(By.CSS_SELECTOR, 'input[aria-label="Answer"]').send_keys('wrong')
        self.driver.find_element(By.CSS_SELECTOR, 'button[type="submit"]').click()
        self.wait.until(lambda d: 'Incorrect. You typed' in d.find_element(By.TAG_NAME, 'body').text)
        self.driver.execute_script('window.blockNext=true')
        assert self.request('/api/cards/2/suppress', 'PUT')[0] == 200
        self.wait.until(lambda d: d.execute_script('return !!window.releaseNext'))
        self.driver.find_element(By.XPATH, "//button[text()='Next']").click()
        assert not self.driver.find_elements(By.CSS_SELECTOR, 'article[data-card-id="2"]')
        self.driver.execute_script('window.blockNext=false;window.releaseNext();window.releaseNext=null')
        self.wait.until(lambda d: d.find_elements(By.CSS_SELECTOR, 'article[data-card-id]') and d.find_element(By.CSS_SELECTOR, 'article[data-card-id]').get_attribute('data-card-id') == '3')

    def test_late_answer_response_cannot_restore_review_after_logout(self):
        self.due_account()
        self.driver.execute_script('window.holdCheck=true')
        self.driver.find_element(By.CSS_SELECTOR, 'input[aria-label="Answer"]').send_keys('wrong')
        self.driver.find_element(By.CSS_SELECTOR, 'button[type="submit"]').click()
        self.wait.until(lambda d: d.execute_script('return !!window.releaseCheck'))
        self.driver.find_element(By.CSS_SELECTOR, 'button[aria-label="Account menu"]').click()
        self.wait.until(lambda d: d.find_element(By.XPATH, "//*[@role='menuitem' and contains(., 'Log Out')]" )).click()
        self.wait.until(lambda d: not d.find_elements(By.CSS_SELECTOR, 'button[aria-label="Account menu"]'))
        self.driver.execute_script('window.releaseCheck()')
        self.wait.until(lambda d: d.execute_script('return window.answerReturned === true'))
        assert not self.driver.find_elements(By.CSS_SELECTOR, 'article[data-card-id]')
        assert 'Incorrect. You typed' not in self.driver.find_element(By.TAG_NAME, 'body').text

    def test_delayed_answer_and_remote_suppression_do_not_restore_a_card(self):
        uid = self.due_account()
        self.driver.execute_script('window.holdCheck=true')
        self.driver.find_element(By.CSS_SELECTOR,'input[aria-label="Answer"]').send_keys('wrong')
        self.driver.find_element(By.CSS_SELECTOR,'button[type="submit"]').click()
        self.wait.until(lambda d:d.execute_script('return !!window.releaseCheck'))
        with sqlite3.connect(self.work/'test.db') as db:
            assert db.execute('SELECT review_revision FROM cards_states WHERE user_id=? AND card_id=1',(uid,)).fetchone()[0]==1
        assert self.request('/api/cards/1/suppress', 'PUT')[0] == 200
        self.driver.execute_script('window.releaseCheck()')
        self.wait.until(lambda d: d.find_elements(By.XPATH, "//button[text()='Next']"))
        self.driver.find_element(By.XPATH, "//button[text()='Next']").click()
        self.wait.until(lambda d: d.find_elements(By.CSS_SELECTOR, 'article[data-card-id]') and d.find_element(By.CSS_SELECTOR, 'article[data-card-id]').get_attribute('data-card-id') != '1')

    def test_retention_is_saved_explicitly_and_reschedules_without_changing_history(self):
        uid = self.due_account()
        self.settings('Review settings')
        thumb=self.wait.until(lambda d:d.find_element(By.CSS_SELECTOR,'[aria-labelledby="desired-retention-label"]')).find_element(By.XPATH,'ancestor::*[@data-slot="slider-thumb"]')
        self.driver.execute_script("arguments[0].scrollIntoView({block:'center'})",thumb)
        before=self.request('/api/user/export')[2]
        with sqlite3.connect(self.work/'test.db') as db:
            before_due=db.execute('SELECT due_at FROM cards_states WHERE user_id=? AND card_id=1',(uid,)).fetchone()[0]
        self.driver.execute_script('window.settingsWrites=[]')
        ActionChains(self.driver).move_to_element(thumb).click_and_hold().move_by_offset(-40,0).pause(0.1).move_by_offset(-40,0).perform()
        assert self.driver.execute_script('return window.settingsWrites.length')==0
        ActionChains(self.driver).release().perform()
        assert self.driver.execute_script('return window.settingsWrites.length')==0
        self.driver.find_element(By.XPATH,"//*[@role='dialog' and .//h2[text()='Settings']]//button[text()='Save']").click()
        self.wait.until(lambda d:d.execute_script('return window.settingsWrites.length')==1)
        self.wait.until(lambda d:not d.find_elements(By.XPATH,"//*[@role='dialog' and .//h2[text()='Settings']]"))
        changed=self.driver.execute_script('return window.settingsWrites[0].desired_retention')
        assert changed < before['settings']['desired_retention']
        self.wait.until(lambda d:self.request('/api/user/settings')[2]['desired_retention']==changed)
        after=self.request('/api/user/export')[2]
        assert before['review_history']==after['review_history']
        with sqlite3.connect(self.work/'test.db') as db:
            after_due=db.execute('SELECT due_at FROM cards_states WHERE user_id=? AND card_id=1',(uid,)).fetchone()[0]
        assert after_due > before_due

    def test_failed_settings_save_keeps_the_draft_and_can_retry(self):
        self.settings('Review settings')
        old_limit=self.request('/api/user/settings')[2]['daily_new_card_limit']
        self.driver.execute_script('window.settingsWrites=[]')
        suppress_label=self.driver.find_element(By.CSS_SELECTOR,'label[for="suppress-new-cards"]')
        self.driver.execute_script("arguments[0].scrollIntoView({block:'center'})",suppress_label)
        suppress_label.click()
        assert self.driver.execute_script('return window.settingsWrites.length')==0
        self.driver.execute_script('window.failSettingsOnce=true')
        self.driver.find_element(By.XPATH,"//*[@role='dialog' and .//h2[text()='Settings']]//button[text()='Save']").click()
        self.wait.until(lambda d: d.find_elements(By.CSS_SELECTOR, '[role="dialog"] [role="alert"]'))
        assert self.request('/api/user/settings')[2]['daily_new_card_limit']==old_limit
        assert self.driver.find_elements(By.XPATH,"//*[@role='dialog' and .//h2[text()='Settings']]")
        self.driver.find_element(By.XPATH,"//*[@role='dialog' and .//h2[text()='Settings']]//button[text()='Save']").click()
        self.wait.until(lambda d:self.request('/api/user/settings')[2]['daily_new_card_limit']!=old_limit)
        self.wait.until(lambda d:not d.find_elements(By.XPATH,"//*[@role='dialog' and .//h2[text()='Settings']]"))

    def test_timed_out_settings_save_keeps_the_dialog_and_server_state(self):
        self.settings('General')
        old_progress=self.request('/api/user/settings')[2]['auto_progress_on_correct']
        progress_label=self.driver.find_element(By.CSS_SELECTOR,'label[for="auto-progress"]')
        self.driver.execute_script("arguments[0].scrollIntoView({block:'center'})",progress_label)
        progress_label.click()
        self.driver.execute_script('window.timeoutSettingsOnce=true')
        self.driver.find_element(By.XPATH,"//*[@role='dialog' and .//h2[text()='Settings']]//button[text()='Save']").click()
        self.wait.until(lambda d:'Saving settings' in d.find_element(By.TAG_NAME,'body').text)
        assert self.driver.find_elements(By.XPATH,"//*[@role='dialog' and .//h2[text()='Settings']]")
        self.wait.until(lambda d:'timed out' in d.find_element(By.TAG_NAME,'body').text)
        assert self.request('/api/user/settings')[2]['auto_progress_on_correct']==old_progress
        self.driver.find_element(By.XPATH,"//*[@role='dialog' and .//h2[text()='Settings']]//button[text()='Cancel']").click()


if __name__ == '__main__':
    unittest.main(verbosity=2)
