#!/usr/bin/perl
# The deterministic "web service" fixture of v0.4 §43.3: one main process that
#
#   * listens on a TCP port on 127.0.0.1              (the listener place, §14.3)
#   * holds a known file open for its whole lifetime  (the process->file edge, §15.5, §44.4)
#   * has several worker children                     ("one service with multiple processes")
#   * accepts and keeps client connections            (the connection place, §14.4, §44.5)
#
# Everything it creates is real kernel state, so the tests that watch it observe real change
# (§43.6) rather than an animation. It exits by itself after `seconds`, so a case can never
# leave a process behind in the image.
#
#   web-service.pl <port> <file-to-hold> <workers> <seconds> [statefile]
#
# It prints `MAIN=<pid> WORKERS=<pids> PORT=<port>` on the first line and, when a statefile is
# given, writes the same values there as shell assignments so a case can `.` them.

use strict;
use warnings;
use IO::Select;
use IO::Socket::INET;

my $port     = shift // 18080;
my $held     = shift // '/etc/hostname';
my $workers  = shift // 2;
my $seconds  = shift // 60;
my $statefile = shift // '';

open(my $handle, '<', $held) or die "fixture: cannot open $held: $!\n";

my $listener = IO::Socket::INET->new(
    LocalAddr => '127.0.0.1',
    LocalPort => $port,
    Proto     => 'tcp',
    Listen    => 16,
    ReuseAddr => 1,
) or die "fixture: cannot listen on 127.0.0.1:$port: $!\n";

my @children;
for my $index (1 .. $workers) {
    my $pid = fork();
    die "fixture: fork failed: $!\n" unless defined $pid;
    if (!$pid) {
        close $listener;
        $0 = "fixture-web-worker-$index";
        sleep $seconds;
        exit 0;
    }
    push @children, $pid;
}

$0 = 'fixture-web-server';
$| = 1;
print "MAIN=$$ WORKERS=@children PORT=$port HELD=$held\n";
if ($statefile ne '') {
    open(my $out, '>', $statefile) or die "fixture: cannot write $statefile: $!\n";
    print $out "FIXTURE_WEB_MAIN=$$\n";
    print $out "FIXTURE_WEB_WORKERS='@children'\n";
    print $out "FIXTURE_WEB_PORT=$port\n";
    print $out "FIXTURE_WEB_HELD=$held\n";
    close $out;
}

# Accept in this process, so the listening socket and every accepted connection belong to the
# pid a case shims as the service main pid. `alarm` interrupts the blocking accept once a
# second, which is how the deadline is honoured without a select loop.
my @accepted;
my $watch = IO::Select->new();
$SIG{ALRM} = sub { };
$SIG{TERM} = sub { exit 0 };
my $deadline = time + $seconds;
while (time < $deadline) {
    alarm 1;
    my $connection = $listener->accept();
    alarm 0;
    if ($connection) {
        push @accepted, $connection;
        $watch->add($connection);
    }
    # A peer that has gone leaves this half readable at end of file, and a server that never
    # noticed would hold the connection in CLOSE_WAIT for as long as it ran. §43.6 asks for the
    # close to be real kernel state rather than a decision a test makes about it, so the
    # connection is closed here exactly when the other end closed it.
    for my $ready ($watch->can_read(0)) {
        my $buffer;
        my $bytes = sysread($ready, $buffer, 4096);
        next if defined $bytes && $bytes > 0;
        $watch->remove($ready);
        @accepted = grep { $_ != $ready } @accepted;
        close $ready;
    }
}
kill 'TERM', @children;
exit 0;
