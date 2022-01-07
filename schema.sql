create table "user" (
    id bigint primary key
);

create table signal (
    user_id bigint not null references "user"(id)
  , url varchar(1024) not null
  , tag varchar(1024) not null
  , signal boolean not null
  , primary key (user_id, url, tag)
);
