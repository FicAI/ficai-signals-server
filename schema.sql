create table user (
    id integer primary key autoincrement
);

create table signal (
    user_id integer not null references user(id)
  , url text not null
  , tag text not null
  , signal int not null
  , primary key (user_id, url, tag)
);
