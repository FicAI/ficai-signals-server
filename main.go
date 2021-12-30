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

type patchQ struct {
	Url   string   `json:"url"`
	Add   []string `json:"add"`
	Rm    []string `json:"rm"`
	Erase []string `json:"erase"`
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

	c.IndentedJSON(http.StatusCreated, q)
}

func main() {
	var err error

	db, err = sql.Open("sqlite3", "file:signals.db?mode=rwc&cache=shared&_locking_mode=EXCLUSIVE&_sync=FULL")
	if err != nil {
		log.Fatal(err)
	}
	defer db.Close()

	router := gin.Default()
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
