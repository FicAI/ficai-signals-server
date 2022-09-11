create sequence account_id_seq as bigint;

create table account (
    id bigint primary key default nextval('account_id_seq')
  , email varchar(256) not null constraint account_email_u unique
  , password_hash varchar(1024) not null
);

alter sequence account_id_seq owned by account.id;

create table session (
    id bytea primary key
  , account_id bigint not null references account(id)
);

create table signal (
    account_id bigint not null references account(id)
  , url varchar(1024) not null
  , tag varchar(1024) not null
  , signal boolean not null
  , primary key (account_id, url, tag)
);
