package main

import (
	"context"
	"database/sql"
	"github.com/gin-gonic/gin"
	_ "github.com/mattn/go-sqlite3"
	"log"
	"net/http"
	"os"
	"os/signal"
	"time"
)

var db *sql.DB

type getQ struct {
	Url string `form:"url"`
}

type getTagInfo struct {
	Tag            string `json:"tag"`
	Signal         *bool  `json:"signal"`
	SignalsFor     int32  `json:"signalsFor"`
	SignalsAgainst int32  `json:"signalsAgainst"`
}

type getA struct {
	Tags []getTagInfo `json:"tags"`
}

type patchQ struct {
	Url   string   `json:"url"`
	Add   []string `json:"add"`
	Rm    []string `json:"rm"`
	Erase []string `json:"erase"`
}

func getSignals(c *gin.Context) {
	uid, err := c.Cookie("FicAiUid")
	if err != nil {
		c.AbortWithError(http.StatusForbidden, err)
		return
	}

	var q getQ
	if err := c.BindQuery(&q); err != nil {
		return
	}
	rows, err := db.Query(
		`
select
	tag,
	sum(iif(signal, 1, 0)) as total_for,
    sum(iif(not signal, 1, 0)) as total_against,
    sum(signal) filter (where user_id = ?) as my_signal
from signal
where url = ?
group by tag
`,
		uid, q.Url,
	)
	if err != nil {
		c.AbortWithError(http.StatusInternalServerError, err)
		return
	}
	defer rows.Close()

	tags := make([]getTagInfo, 0)
	for rows.Next() {
		var (
			tag           string
			total_for     int32
			total_against int32
			my_signal     sql.NullBool
		)
		if err := rows.Scan(&tag, &total_for, &total_against, &my_signal); err != nil {
			c.AbortWithError(http.StatusInternalServerError, err)
			return
		}
		tagInfo := getTagInfo{
			Tag:            tag,
			Signal:         nil,
			SignalsFor:     total_for,
			SignalsAgainst: total_against,
		}
		if my_signal.Valid {
			tagInfo.Signal = &my_signal.Bool
		}
		tags = append(tags, tagInfo)
	}
	c.IndentedJSON(http.StatusOK, getA{tags})
}

func patchSignals(c *gin.Context) {
	uid, err := c.Cookie("FicAiUid")
	if err != nil {
		c.AbortWithError(http.StatusForbidden, err)
		return
	}

	var q patchQ
	if err := c.BindJSON(&q); err != nil {
		return
	}

	log.Printf("'%s' %v\n", uid, q)

	if q.Add != nil {
		for _, tag := range q.Add {
			if _, err := db.Exec(
				"insert or replace into signal (user_id, url, tag, signal) values (?, ?, ?, ?)",
				uid,
				q.Url,
				tag,
				true,
			); err != nil {
				c.AbortWithError(http.StatusInternalServerError, err)
				return
			}
		}
	}
	if q.Rm != nil {
		for _, tag := range q.Rm {
			if _, err := db.Exec(
				"insert or replace into signal (user_id, url, tag, signal) values (?, ?, ?, ?)",
				uid,
				q.Url,
				tag,
				false,
			); err != nil {
				c.AbortWithError(http.StatusInternalServerError, err)
				return
			}
		}
	}
	if q.Erase != nil {
		for _, tag := range q.Erase {
			if _, err := db.Exec(
				"delete from signal where user_id = ? and url = ? and tag = ?",
				uid,
				q.Url,
				tag,
			); err != nil {
				c.AbortWithError(http.StatusInternalServerError, err)
				return
			}
		}
	}

	c.AbortWithStatus(http.StatusNoContent)
}

func main() {
	var err error

	db, err = sql.Open("sqlite3", "file:signals.db?mode=rwc&cache=shared&_locking_mode=EXCLUSIVE&_sync=FULL")
	if err != nil {
		log.Fatal(err)
	}
	defer db.Close()

	router := gin.Default()
	router.GET("/v1/signals", getSignals)
	router.PATCH("/v1/signals", patchSignals)

	srv := &http.Server{
		Addr:    "localhost:8080",
		Handler: router,
	}

	go func() {
		// service connections
		if err := srv.ListenAndServe(); err != nil {
			log.Printf("listen: %s\n", err)
		}
	}()

	quit := make(chan os.Signal)
	signal.Notify(quit, os.Interrupt)
	<-quit
	log.Println("shutting down server")

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if err := srv.Shutdown(ctx); err != nil {
		log.Fatal("Server Shutdown:", err)
	}
	log.Println("Server exiting")
}
