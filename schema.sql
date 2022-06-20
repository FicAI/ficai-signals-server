create extension if not exists fuzzystrmatch;

create sequence user_id_seq as bigint;

create table "user" (
    id bigint primary key default nextval('user_id_seq')
  , email varchar(256) not null constraint user_email_u unique
  , password_hash varchar(1024) not null
);

alter sequence user_id_seq owned by "user".id;

create table session (
    id bytea primary key
  , user_id bigint not null references "user"(id)
);

create table signal (
    user_id bigint not null references "user"(id)
  , url varchar(1024) not null
  , tag varchar(1024) not null
  , signal boolean not null
  , primary key (user_id, url, tag)
);
