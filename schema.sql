create extension if not exists fuzzystrmatch;

create sequence if not exists user_id_seq as bigint;

create table if not exists "user" (
    id bigint primary key default nextval('user_id_seq')
  , email varchar(256) not null constraint user_email_u unique
  , password_hash varchar(1024) not null
);

alter sequence user_id_seq owned by "user".id;

create table if not exists session (
    id bytea primary key
  , user_id bigint not null references "user"(id)
);

create table if not exists fic (
    id varchar(1024) primary key
  , url varchar(1024) not null
  , title text not null
);

create table if not exists fic_url_cache (
    url varchar(1024)
  , fic_id varchar(1024) not null references "fic"(id)
  , fetched timestamptz
  , primary key (url, fic_id)
);

create table if not exists signal (
    user_id bigint not null references "user"(id)
  , fic_id varchar(1024) not null references "fic"(id)
  , tag varchar(1024) not null
  , signal boolean not null
  , primary key (user_id, fic_id, tag)
);
