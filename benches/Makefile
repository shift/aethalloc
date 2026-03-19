CC ?= gcc
CFLAGS ?= -O3 -pthread -Wall -Wextra

BENCHMARKS = packet_churn tail_latency producer_consumer fragmentation multithread_churn \
              micro_burst rss_reclaim asymmetric_threads kv_store massive_alloc

all: $(BENCHMARKS)

packet_churn: packet_churn.c
	$(CC) $(CFLAGS) -o $@ $<

tail_latency: tail_latency.c
	$(CC) $(CFLAGS) -o $@ $<

producer_consumer: producer_consumer.c
	$(CC) $(CFLAGS) -o $@ $<

fragmentation: fragmentation.c
	$(CC) $(CFLAGS) -o $@ $<

multithread_churn: multithread_churn.c
	$(CC) $(CFLAGS) -o $@ $<

micro_burst: micro_burst.c
	$(CC) $(CFLAGS) -o $@ $<

rss_reclaim: rss_reclaim.c
	$(CC) $(CFLAGS) -o $@ $<

asymmetric_threads: asymmetric_threads.c
	$(CC) $(CFLAGS) -o $@ $<

kv_store: kv_store.c
	$(CC) $(CFLAGS) -o $@ $<

massive_alloc: massive_alloc.c
	$(CC) $(CFLAGS) -o $@ $<

clean:
	rm -f $(BENCHMARKS)

.PHONY: all clean
