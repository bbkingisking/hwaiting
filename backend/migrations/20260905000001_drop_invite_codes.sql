-- Demo deployment: this instance allows free sign-up, so the invite-code
-- gate (auth::check_invite_code/consume_invite_code, the admin invite
-- endpoints, and the invite fields on both signup flows) has been removed
-- entirely rather than merely bypassed - see the commit removing it. This
-- table was the only thing keeping the previous state, and nothing else in
-- the schema references it (used_by_user_id was the only FK, users -> here,
-- never the other way), so dropping it is a plain DROP TABLE.

DROP TABLE IF EXISTS invite_codes;
